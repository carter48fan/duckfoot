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
}

const FLAG_WB:       u32 = 1u;
const FLAG_EXPOSURE: u32 = 2u;
const FLAG_CURVE:    u32 = 4u;
const FLAG_FILMIC:   u32 = 8u;
const FLAG_BALANCE:  u32 = 16u;

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

// Where a photosite stops carrying colour information. Demosaicing averages neighbours,
// so a clipped site lands slightly under 1.0; the ramp starts early enough to catch that
// without desaturating legitimately bright colour.
const CLIP_LO: f32 = 0.93;
const CLIP_HI: f32 = 1.0;

// Highlight reconstruction.
//
// A photosite saturates at 1.0 after black/white normalisation, so a blown specular reads
// (1,1,1) there no matter what colour it actually was. White balance then multiplies those
// three equal values by three unequal gains — [2.03, 1.00, 1.80] on a Sony A7 III — and a
// neutral highlight comes out magenta. That is not a white balance error; it is a channel
// being asked to report a value it never measured.
//
// A saturated photosite carries no colour, so the only defensible colour is neutral.
// Collapsing toward the minimum balanced channel does that, and it is self-normalising: an
// unclipped neutral pixel already has three equal channels, so this is a no-op everywhere
// except where the sensor actually ran out of range.
//
// This reconstructs colour, not detail. Texture inside a blown highlight is gone at capture
// and no amount of arithmetic here brings it back.
fn reconstruct_highlights(balanced: vec3<f32>, sensor_peak: f32) -> vec3<f32> {
    let t = smoothstep(CLIP_LO, CLIP_HI, sensor_peak);
    if (t <= 0.0) {
        return balanced;
    }
    let neutral = vec3<f32>(min(balanced.r, min(balanced.g, balanced.b)));
    return mix(balanced, neutral, t);
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
    let src = sensor_coords(pos.xy);

    // Straightening rotates the corners outside the sensor. Those pixels were never
    // photographed, so they are black rather than a smeared edge clamp.
    if (src.x < 0.0 || src.y < 0.0 || src.x >= u.sizes.z || src.y >= u.sizes.w) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Sampled rather than loaded, because straightening lands between texels. With no
    // straighten the coordinates fall on texel centres and this is exact.
    var c = textureSampleLevel(sensor, sensor_sampler, src / u.sizes.zw, 0.0).rgb;

    // Captured before the gains are applied: saturation is a property of the sensor, and
    // after white balance there is no longer any way to tell a clipped channel from a
    // merely bright one.
    let sensor_peak = max(c.r, max(c.g, c.b));

    // White balance is applied in camera space, BEFORE the primaries conversion.
    if (enabled(FLAG_WB)) {
        c = c * u.wb.rgb;
    }

    c = reconstruct_highlights(c, sensor_peak);

    // Camera-native primaries -> linear sRGB. Never optional: skipping this is what made
    // every image in the last build look like a broken white balance.
    c = vec3<f32>(dot(u.cam0.rgb, c), dot(u.cam1.rgb, c), dot(u.cam2.rgb, c));

    if (enabled(FLAG_EXPOSURE)) {
        c = (c - vec3<f32>(u.exposure.y)) * u.exposure.x;
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

    return vec4<f32>(linear_to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}
