// Pass 2: the whole adjustment chain, fused into one fragment shader.
//
// This is the performance decision the product thesis pays for. Because the module stack
// is fixed-order and not rearrangeable (DESIGN.md, Barrel 1), the chain is known ahead of
// time and can be fused: ONE texture read and ONE write per pixel for the entire stack,
// instead of a VRAM round trip per module.
//
// Disabled modules are uniform-flag branches, not shader permutations. The branch is
// coherent across the whole draw — every invocation takes the same side — so it is
// effectively free, and it keeps "one render path" (invariant 2) actually verifiable.
//
// A slider drag rewrites `Adjust` and nothing else. No re-decode, no re-upload.

struct Adjust {
    cam0:     vec4<f32>,   // camera RGB -> linear sRGB, row 0 (xyz used)
    cam1:     vec4<f32>,
    cam2:     vec4<f32>,
    wb:       vec4<f32>,   // xyz = neutral multipliers
    exposure: vec4<f32>,   // x = gain (2^EV), y = black point
    filmic:   vec4<f32>,   // x = white EV, y = black EV, z = latitude, w = contrast
    lift:     vec4<f32>,
    gamma:    vec4<f32>,
    gain:     vec4<f32>,
    offset:   vec4<f32>,
    flags:    vec4<u32>,   // x = enabled bitmask
    sizes:    vec4<f32>,   // xy = output size, zw = sensor size
    orient:   vec4<f32>,   // xy = cos/sin of the INVERSE straighten, zw = mirror signs
    turns:    vec4<u32>,   // x = quarter turns clockwise
    split:    vec4<f32>,   // x = split fraction 0.0..1.0, or < 0.0 if disabled
    as_shot_wb: vec4<f32>, // xyz = as-shot camera neutral multipliers
    tonal:      vec4<f32>, // x = highlights, y = shadows, z = whites, w = blacks
    vibrance:   vec4<f32>, // x = vibrance, y = saturation
    vignette:   vec4<f32>, // x = amount, y = radius
    sharpen:    vec4<f32>, // x = amount
    dest:       vec4<f32>, // xy = render target size, z = sensor mip level to sample
}

const FLAG_WB:       u32 = 1u;
const FLAG_EXPOSURE: u32 = 2u;
const FLAG_CURVE:    u32 = 4u;
const FLAG_FILMIC:   u32 = 8u;
const FLAG_BALANCE:  u32 = 16u;
const FLAG_TONAL:    u32 = 32u;
const FLAG_VIBRANCE: u32 = 64u;
const FLAG_VIGNETTE: u32 = 128u;
const FLAG_SHARPEN:  u32 = 256u;

@group(0) @binding(0) var sensor: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: Adjust;
@group(0) @binding(2) var curve_lut: texture_2d<f32>;
@group(0) @binding(3) var sensor_sampler: sampler;

const LUT_MAX: i32 = 1023;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn enabled(bit: u32) -> bool {
    return (u.flags.x & bit) != 0u;
}

fn lut_at(i: i32) -> f32 {
    return textureLoad(curve_lut, vec2<i32>(clamp(i, 0, LUT_MAX), 0), 0).r;
}

// The LUT is 1024x1 r32float, built on the CPU with a monotone cubic (Fritsch-Carlson)
// spline. Interpolated by hand rather than by a sampler: linear filtering of r32float is
// an optional WebGPU feature (FLOAT32_FILTERABLE) that plenty of Android GPUs lack, and
// textureLoad has no such requirement.
//
// Values above 1.0 are headroom and pass through with the curve's top slope rather than
// being clamped here — clamping mid-chain is what destroys highlight recovery.
fn curve(v: f32) -> f32 {
    if (v <= 0.0) {
        return lut_at(0);
    }
    if (v >= 1.0) {
        return lut_at(LUT_MAX) + (v - 1.0);
    }
    let p = v * f32(LUT_MAX);
    let i = i32(floor(p));
    return mix(lut_at(i), lut_at(i + 1), p - floor(p));
}

fn filmic_channel(v: f32) -> f32 {
    if (v <= 0.0) {
        return 0.0;
    }
    let white = exp2(u.filmic.x);
    let black = exp2(u.filmic.y);
    let latitude = max(u.filmic.z / 100.0, 0.01);
    let log_range = max(log2(white / black), 0.001);
    let norm = (log2(v / black) / log_range - 0.5) * u.filmic.w;
    return clamp(1.0 / (1.0 + exp(-norm / latitude)), 0.0, 1.0);
}

// The one and only transfer function in the pipeline. The render target is a plain
// unorm format, so nothing else applies or removes a gamma — see OUTPUT_FORMAT.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0031308)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// The sensor texture holds each pixel in two parts (demosaic_rb.wgsl, `reconstruct`): rgb,
// the camera colour still trusted, and a, the neutral standing in for whatever clipped.
// White balance scales the first and must not touch the second, or a blown highlight takes
// on the balance's tint.
fn balanced(s: vec4<f32>, wb: vec3<f32>) -> vec3<f32> {
    return s.rgb * wb + vec3<f32>(s.a);
}

// Orientation, by inverse mapping.
//
// The fragment knows where it is in the OUTPUT and has to find where that came from in the
// sensor, so every step is applied backwards and in reverse order: straighten, then mirror,
// then the right-angle turns.
//
// Right-angle turns swap the output dimensions; straightening does not, so straightening
// rotates the corners out of frame and a crop is expected to take them. That is the normal
// bargain and the alternative — growing the canvas — invents pixels that were never
// photographed.
fn sensor_coords(out_px: vec2<f32>) -> vec2<f32> {
    let out_size = u.sizes.xy;
    let sensor_size = u.sizes.zw;

    // Centred, so rotation is about the middle of the frame rather than a corner.
    var p = out_px - out_size * 0.5;

    // Undo the fine straighten.
    let c = u.orient.x;
    let s = u.orient.y;
    p = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);

    // Undo mirroring. Signs are -1 when that axis was flipped.
    p = p * u.orient.zw;

    // Undo the right-angle turns. Each clockwise turn maps (x, y) -> (-y, x), so the
    // inverse of one turn is (x, y) -> (y, -x).
    for (var i = 0u; i < u.turns.x; i = i + 1u) {
        p = vec2<f32>(p.y, -p.x);
    }

    return p + sensor_size * 0.5;
}

fn balance_channel(v: f32, lift: f32, gma: f32, gn: f32, off: f32) -> f32 {
    var o = v + off;
    o = o * (1.0 + gn) + lift * (1.0 - o);
    if (o > 0.0) {
        o = pow(o, 1.0 / max(1.0 + gma, 0.01));
    }
    return o;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    // The target is either the full-resolution export or a reduced preview. Everything
    // below works in full-resolution output pixels, so both compute the same image and
    // differ only in how densely it is sampled (DESIGN.md: preview and export).
    let out_px = pos.xy * u.sizes.xy / u.dest.xy;
    let lod = u.dest.z;
    let src = sensor_coords(out_px);

    // Straightening rotates the corners outside the sensor. Those pixels were never
    // photographed, so they are black rather than a smeared edge clamp.
    if (src.x < 0.0 || src.y < 0.0 || src.x >= u.sizes.z || src.y >= u.sizes.w) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Sampled rather than loaded, because straightening lands between texels. With no
    // straighten the coordinates fall on texel centres and this is exact.
    let uv = src / u.sizes.zw;
    var s = textureSampleLevel(sensor, sensor_sampler, uv, lod);

    if (enabled(FLAG_SHARPEN)) {
        // One texel of whichever level is being sampled, so a reduced preview sharpens at
        // its own scale. That is an approximation of the export, and the only one.
        // All four channels: the neutral in `a` is part of the pixel's light too.
        let step = exp2(lod) / u.sizes.zw;
        let s_up = textureSampleLevel(sensor, sensor_sampler, uv - vec2<f32>(0.0, step.y), lod);
        let s_dn = textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(0.0, step.y), lod);
        let s_lf = textureSampleLevel(sensor, sensor_sampler, uv - vec2<f32>(step.x, 0.0), lod);
        let s_rt = textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(step.x, 0.0), lod);
        let laplacian = s * 4.0 - (s_up + s_dn + s_lf + s_rt);
        s = clamp(s + laplacian * u.sharpen.x, vec4<f32>(0.0), vec4<f32>(2.0));
    }

    let split_pos = u.split.x;
    let is_split = split_pos >= 0.0;
    if (is_split) {
        // The divider itself is drawn by the UI, at screen resolution, so it stays one
        // line wide at every zoom instead of scaling with the image.
        let norm_x = out_px.x / u.sizes.x;
        // "Before" baseline on the left side of the split: as-shot WB, highlight recovery, camera matrix -> sRGB
        if (norm_x < split_pos) {
            var bc = balanced(s, u.as_shot_wb.rgb);
            bc = vec3<f32>(dot(u.cam0.rgb, bc), dot(u.cam1.rgb, bc), dot(u.cam2.rgb, bc));
            return vec4<f32>(linear_to_srgb(clamp(bc, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
        }
    }

    // White balance is applied in camera space, BEFORE the primaries conversion.
    var c = balanced(s, vec3<f32>(1.0));
    if (enabled(FLAG_WB)) {
        c = balanced(s, u.wb.rgb);
    }

    // Camera-native primaries -> linear sRGB. Never optional: skipping this is what made
    // every image in the last build look like a broken white balance.
    c = vec3<f32>(dot(u.cam0.rgb, c), dot(u.cam1.rgb, c), dot(u.cam2.rgb, c));

    if (enabled(FLAG_EXPOSURE)) {
        c = (c - vec3<f32>(u.exposure.y)) * u.exposure.x;
    }

    if (enabled(FLAG_TONAL)) {
        let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
        let s_weight = 1.0 - smoothstep(0.0, 0.5, luma);
        let h_weight = smoothstep(0.35, 1.0, luma);
        let w_weight = smoothstep(0.7, 1.2, luma);
        let b_weight = 1.0 - smoothstep(0.0, 0.2, luma);

        let s_factor = 1.0 + u.tonal.y * s_weight * 0.75;
        let h_factor = 1.0 + u.tonal.x * h_weight * 0.6;
        let w_factor = 1.0 + u.tonal.z * w_weight * 0.5;
        let b_offset = u.tonal.w * b_weight * 0.05;

        c = max(vec3<f32>(0.0), (c + vec3<f32>(b_offset)) * max(s_factor * h_factor * w_factor, 0.01));
    }

    if (enabled(FLAG_CURVE)) {
        c = vec3<f32>(curve(c.r), curve(c.g), curve(c.b));
    }

    if (enabled(FLAG_FILMIC)) {
        c = vec3<f32>(filmic_channel(c.r), filmic_channel(c.g), filmic_channel(c.b));
    }

    if (enabled(FLAG_BALANCE)) {
        c = vec3<f32>(
            balance_channel(c.r, u.lift.r, u.gamma.r, u.gain.r, u.offset.r),
            balance_channel(c.g, u.lift.g, u.gamma.g, u.gain.g, u.offset.g),
            balance_channel(c.b, u.lift.b, u.gamma.b, u.gain.b, u.offset.b),
        );
    }

    if (enabled(FLAG_VIBRANCE)) {
        let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
        let max_ch = max(c.r, max(c.g, c.b));
        let min_ch = min(c.r, min(c.g, c.b));
        let cur_sat = (max_ch - min_ch) / max(max_ch, 0.001);
        let vib_boost = u.vibrance.x * (1.0 - clamp(cur_sat, 0.0, 1.0));
        let sat_factor = max(1.0 + u.vibrance.y + vib_boost, 0.0);
        c = mix(vec3<f32>(luma), c, sat_factor);
    }

    if (enabled(FLAG_VIGNETTE)) {
        let norm_pos = out_px / u.sizes.xy - vec2<f32>(0.5, 0.5);
        let dist = length(norm_pos) * 1.4142;
        let rad = max(u.vignette.y, 0.05);
        let v_falloff = smoothstep(rad * 0.4, rad, dist);
        c = c * max(1.0 + u.vignette.x * v_falloff, 0.0);
    }

    return vec4<f32>(linear_to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}
