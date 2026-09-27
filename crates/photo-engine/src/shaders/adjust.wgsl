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
    cam0:       vec4<f32>,   // camera RGB -> linear sRGB, row 0 (xyz used)
    cam1:       vec4<f32>,
    cam2:       vec4<f32>,
    wb:         vec4<f32>,   // xyz = neutral multipliers
    exposure:   vec4<f32>,   // x = gain (2^EV), y = black point, z = rolloff, w = tau
    filmic:     vec4<f32>,   // x = white EV, y = black EV, z = latitude, w = contrast
    lift:       vec4<f32>,
    gamma:      vec4<f32>,
    gain:       vec4<f32>,
    offset:     vec4<f32>,
    flags:      vec4<u32>,   // x = enabled bitmask, y = 1 if the curve works on display values,
                             // z = 1 to dither for an 8-bit target
    sizes:      vec4<f32>,   // xy = output size, zw = sensor size
    orient:     vec4<f32>,   // xy = cos/sin of the INVERSE straighten, zw = mirror signs
    turns:      vec4<u32>,   // x = quarter turns clockwise
    split:      vec4<f32>,   // x = split fraction 0.0..1.0, or < 0.0 if disabled
    as_shot_wb: vec4<f32>,   // xyz = as-shot camera neutral multipliers
    tonal:      vec4<f32>,   // x = highlights, y = shadows, z = whites, w = blacks
    vibrance:   vec4<f32>,   // x = vibrance, y = saturation
    vignette:   vec4<f32>,   // x = amount, y = radius
    sharpen:    vec4<f32>,   // x = amount
    dest:       vec4<f32>,   // xy = render target size, z = sensor mip level to sample
    detail:     vec4<f32>,   // x = clarity, y = texture, z = dehaze
    eq_red:     vec4<f32>,   // x = hue (-30..30 deg), y = sat (-100..100), z = lum (-100..100)
    eq_orange:  vec4<f32>,
    eq_yellow:  vec4<f32>,
    eq_green:   vec4<f32>,
    eq_aqua:    vec4<f32>,
    eq_blue:    vec4<f32>,
    eq_purple:  vec4<f32>,
    eq_magenta: vec4<f32>,
    frame:      vec4<f32>,   // the crop in the oriented frame: x, y, width, height (0..1)
    lens:       vec4<f32>,   // x = radial distortion k, y = fill zoom, z/w = red/blue CA scale
    lens_b:     vec4<f32>,   // x = vignetting correction, stops at the corner
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
const FLAG_CLARITY:  u32 = 512u;
const FLAG_DEHAZE:   u32 = 1024u;
const FLAG_COLOR_EQ: u32 = 2048u;
const FLAG_LENS:     u32 = 4096u;
// Tests only: paint any pixel that went NaN or infinite anywhere in the chain transparent
// magenta, so a pixel test can see a numerics bug instead of a black speck
// (`Engine::set_nan_paint`).
const FLAG_NAN_PAINT: u32 = 0x80000000u;

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

// NaN or infinity, read off the bit pattern (all exponent bits set). WGSL lets a compiler
// assume neither ever occurs, so the usual `x != x` may simply be folded to false.
fn not_finite(c: vec3<f32>) -> bool {
    let exponent = bitcast<vec3<u32>>(c) & vec3<u32>(0x7f800000u);
    return any(exponent == vec3<u32>(0x7f800000u));
}

fn lut_at(i: i32) -> f32 {
    return textureLoad(curve_lut, vec2<i32>(clamp(i, 0, LUT_MAX), 0), 0).r;
}

// The LUT is 1024x1 r32float, built on the CPU with a monotone cubic (Fritsch-Carlson)
// spline. Interpolated by hand rather than by a sampler: linear filtering of r32float is
// an optional WebGPU feature (FLOAT32_FILTERABLE) that plenty of Android GPUs lack, and
// textureLoad has no such requirement.
//
// Values above 1.0 are headroom and pass through at a slope of 1 above the curve's top
// rather than being clamped here — clamping mid-chain is what destroys highlight recovery.
// The curve with its axes in display values: what the histogram under the editor shows,
// and where a quarter-tone node sits on a quarter tone. Encoded, looked up, decoded.
fn curve_display(v: f32) -> f32 {
    var e = 0.0;
    if (v > 0.0031308) {
        e = 1.055 * pow(v, 1.0 / 2.4) - 0.055;
    } else if (v > 0.0) {
        e = v * 12.92;
    }
    return srgb_to_linear1(curve(e));
}

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

// Mid grey, and what the display shows it as with filmic off: sRGB-encoded 0.18, 118/255.
const MID_GREY: f32 = 0.18;
const MID_GREY_DISPLAY: f32 = 0.46135;

fn sigmoid(x: f32) -> f32 {
    return 1.0 / (1.0 + exp(-x));
}

fn srgb_to_linear1(v: f32) -> f32 {
    if (v <= 0.04045) {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

// Filmic: a sigmoid across the scene's range in stops, black_ev..white_ev, onto the display.
//
// The curve is shaped in display terms and its result is decoded back to linear light, so
// the `linear_to_srgb` at the end of the chain stays the only encode. Treating the display
// value as linear — as this used to — encoded it twice: mid grey showed at 184/255 and
// black_ev at 69, the flat grey look that was blamed on latitude.
//
// Normalised so black_ev lands exactly on 0 and white_ev on 1, then bent by an output power
// so that mid grey lands where it does with filmic off. Contrast and latitude shape the S
// around it; they no longer move it.
fn filmic_curve(v: f32) -> f32 {
    let black_ev = u.filmic.y;
    let range = max(u.filmic.x - black_ev, 0.01);
    // Steepness: contrast over latitude (a percentage of the range), as before.
    let k = max(u.filmic.w, 0.01) / max(u.filmic.z / 100.0, 0.01);

    // Centred on mid grey, so contrast is symmetric about it.
    let x_grey = clamp((log2(MID_GREY) - black_ev) / range, 0.001, 0.999);
    let s0 = sigmoid(-k * x_grey);
    let s1 = sigmoid(k * (1.0 - x_grey));
    let span = max(s1 - s0, 1e-6);

    let x = clamp((log2(max(v, 1e-10)) - black_ev) / range, 0.0, 1.0);
    let y = clamp((sigmoid(k * (x - x_grey)) - s0) / span, 0.0, 1.0);
    let y_grey = clamp((0.5 - s0) / span, 1e-4, 1.0 - 1e-4);
    let power = clamp(log2(MID_GREY_DISPLAY) / log2(y_grey), 0.2, 5.0);
    return srgb_to_linear1(pow(max(y, 1e-7), power));
}

// Filmic on the pixel's brightness, not on each channel: the curve maps max(R, G, B) and all
// three channels scale by the same ratio. Per channel, a saturated highlight's channels were
// compressed by different amounts and its hue slid toward the nearest primary.
fn filmic(c: vec3<f32>) -> vec3<f32> {
    let norm = max(c.r, max(c.g, c.b));
    if (norm <= 0.0) {
        return vec3<f32>(0.0);
    }
    return c * (filmic_curve(norm) / norm);
}

// The one and only transfer function in the pipeline. The render target is a plain
// unorm format, so nothing else applies or removes a gamma — see OUTPUT_FORMAT.
// The finished pixel: display-encoded, and for an 8-bit target dithered by up to half a
// level either way, so a smooth gradient (a sky) lands on a fine mix of neighbouring levels
// instead of visible steps. The noise (interleaved gradient noise) is fixed per output
// pixel, so the preview at 1:1 and the export agree exactly, and has a mean of zero, so it
// shifts no tone. A 16-bit target has levels to spare and is left alone.
fn finish(linear: vec3<f32>, out_px: vec2<f32>) -> vec4<f32> {
    var e = linear_to_srgb(clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0)));
    if (u.flags.z == 1u) {
        let p = floor(out_px);
        let n = fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
        e = clamp(e + (n - 0.5) / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(e, 1.0);
}

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

    // Undo the lens: `p` is now where the point belongs on an ideal sensor, and a lens
    // with radial distortion put it further out or further in. About the sensor's centre,
    // the optical axis for every lens without shift.
    if (enabled(FLAG_LENS)) {
        p = lens_distorted(p, sensor_size);
    }

    return p + sensor_size * 0.5;
}

// The one-term radial model, r' = r·(1 + k·r²), with r a fraction of the half-diagonal.
// `zoom` first, which for a pincushion correction (k > 0) scales the frame so the corners
// land on the sensor's corners instead of past them: no pixel is invented.
fn lens_distorted(p: vec2<f32>, sensor_size: vec2<f32>) -> vec2<f32> {
    let q = p * u.lens.y;
    let r = length(q) / length(sensor_size * 0.5);
    return q * (1.0 + u.lens.x * r * r);
}

// What the vignetting correction gains a pixel by, from where it lies on the sensor.
fn lens_gain_at(src: vec2<f32>) -> f32 {
    let half = u.sizes.zw * 0.5;
    let r = length(src - half) / length(half);
    return exp2(u.lens_b.x * r * r);
}

// Lateral chromatic aberration: the lens images red and blue at slightly different sizes
// from green. Each is sampled from where its own size put it, about the optical axis.
fn lens_ca(s: vec4<f32>, src: vec2<f32>, lod: f32) -> vec4<f32> {
    let half = u.sizes.zw * 0.5;
    let uv_r = ((src - half) * u.lens.z + half) / u.sizes.zw;
    let uv_b = ((src - half) * u.lens.w + half) / u.sizes.zw;
    let red = textureSampleLevel(sensor, sensor_sampler, uv_r, lod).r;
    let blue = textureSampleLevel(sensor, sensor_sampler, uv_b, lod).b;
    return vec4<f32>(red, s.g, blue, s.a);
}

// The vignetting correction's gain at this pixel. Neighbourhood estimates (clarity,
// dehaze) sample the sensor as it is and scale by this, so a corrected corner does not
// read as local contrast. It varies over hundreds of pixels, so the pixel's gain stands
// in for its neighbourhood's.
var<private> lens_gain: f32 = 1.0;

// A sensor sample as linear sRGB: white balance in camera space, then the primaries.
fn scene(s: vec4<f32>, wb: vec3<f32>) -> vec3<f32> {
    let c = balanced(s, wb);
    return vec3<f32>(dot(u.cam0.rgb, c), dot(u.cam1.rgb, c), dot(u.cam2.rgb, c));
}

// Linear exposure only — no highlight rolloff. For neighbourhood estimates, which need to
// know roughly how bright the surroundings will be, not the exact tone.
fn exposed(c: vec3<f32>) -> vec3<f32> {
    if (enabled(FLAG_EXPOSURE)) {
        return max(vec3<f32>(0.0), (c - vec3<f32>(u.exposure.y)) * u.exposure.x);
    }
    return c;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The sensor around `uv`, blurred to the scale of mip `level`: nine bilinear taps a texel
// apart, weighted 1-2-1, so a mip reads as a smooth bump instead of the 2^level-pixel
// blocks it is stored in — which would otherwise show up as a grid in every halo.
//
// Levels are absolute (level 3 is 8 full-resolution pixels at every preview size), so the
// neighbourhoods are the same size in the preview as in the export.
fn blurred(uv: vec2<f32>, level: f32) -> vec4<f32> {
    let t = exp2(level) / u.sizes.zw;
    var sum = textureSampleLevel(sensor, sensor_sampler, uv, level) * 4.0;
    sum += textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(t.x, 0.0), level) * 2.0;
    sum += textureSampleLevel(sensor, sensor_sampler, uv - vec2<f32>(t.x, 0.0), level) * 2.0;
    sum += textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(0.0, t.y), level) * 2.0;
    sum += textureSampleLevel(sensor, sensor_sampler, uv - vec2<f32>(0.0, t.y), level) * 2.0;
    sum += textureSampleLevel(sensor, sensor_sampler, uv + t, level);
    sum += textureSampleLevel(sensor, sensor_sampler, uv - t, level);
    sum += textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(t.x, -t.y), level);
    sum += textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(-t.x, t.y), level);
    return sum / 16.0;
}

// Clarity and texture, as a gain on the pixel.
//
// Both scale how far a pixel's brightness sits from its neighbourhood's, measured in stops
// (log2 luminance) rather than linear light. In linear light the same local contrast is a
// hundred times larger in a highlight than in a shadow, so a linear boost does nothing
// visible to most of the picture and then blows out the bright edges. As a ratio on all
// three channels it changes contrast and never hue.
//
// Texture is the fine band: the pixel against ~4 px around it. Clarity is the coarse one:
// ~4 px against ~40 px, weighted to the midtones the way the eye reads "punch". Negative
// amounts flatten the band instead, which is the soft-skin / glow direction.
fn local_contrast(s: vec4<f32>, uv: vec2<f32>, lod: f32, wb: vec3<f32>) -> f32 {
    // Stops, floored so that noise in the deep shadows is not read as detail.
    let floor_y = 0.002;
    // Clarity's bands are absolute levels (~4 px against ~32 px), the same at every zoom,
    // so a reduced preview boosts what the export will. Texture's band — the pixel against
    // ~4 px — is finer than a preview at fit can show; what the downscaled export keeps of
    // it is a shift in each region's level that no mip records. The nearest thing the
    // preview has is its own finest band, which measures closer to the export than
    // dropping the band does (pinned by the GPU test `a_reduced_preview_shows_…`).
    let fine_level = max(2.0, lod + 1.0);
    let coarse_level = max(5.0, fine_level);

    // A saturated blue or violet comes out of the camera matrix with a negative channel,
    // and its luminance can land below zero: its log is NaN, which survives every clamp
    // after it and ends as a black speck. Luminance below zero is none at all.
    let y0 = max(luma(scene(s, wb)), 0.0) + floor_y;
    let y_fine = max(luma(scene(blurred(uv, fine_level), wb)) * lens_gain, 0.0) + floor_y;
    let y_coarse = max(luma(scene(blurred(uv, coarse_level), wb)) * lens_gain, 0.0) + floor_y;

    // Halos grow with the size of an edge; past two stops, more boost is only more halo.
    let d_texture = clamp(log2(y0 / y_fine), -2.0, 2.0);
    let d_clarity = clamp(log2(y_fine / y_coarse), -2.0, 2.0);

    // Midtone weight from how bright the neighbourhood will be once exposed.
    let l = sqrt(clamp(luma(exposed(vec3<f32>(y_coarse))), 0.0, 1.0));
    let mid = mix(0.35, 1.0, 1.0 - (2.0 * l - 1.0) * (2.0 * l - 1.0));

    // -100 removes 90% of a band; +100 roughly doubles it.
    let k_clarity = u.detail.x * select(0.009, 0.011, u.detail.x > 0.0) * mid;
    let k_texture = u.detail.y * select(0.009, 0.011, u.detail.y > 0.0);
    return exp2(k_clarity * d_clarity + k_texture * d_texture);
}

// Dehaze, by the atmospheric model: what reached the camera is the scene attenuated by the
// air, plus a veil of scattered light, I = J·t + A·(1 − t). Removing haze solves for J.
//
// The veil is estimated from the neighbourhood (~40 px), not the pixel: its darkest channel
// is how far haze has lifted the blacks there, since clear air leaves something in every
// patch near black in at least one channel. Estimating it per pixel — as this used to — is
// just a contrast curve that crushes every dark pixel and tints the rest.
//
// Worked in a display-like encoding, where haze reads as the lifted, flat look it is; in
// linear light the same algebra pushes a midtone to black at modest strengths.
fn dehaze(c: vec3<f32>, uv: vec2<f32>, wb: vec3<f32>) -> vec3<f32> {
    let amount = u.detail.z * 0.01;
    let gamma = 1.0 / 2.2;
    let air = 1.0;
    let p = pow(max(c, vec3<f32>(0.0)), vec3<f32>(gamma));

    if (amount < 0.0) {
        // Adding haze: a pale veil laid evenly over everything.
        let veil = -amount * 0.45;
        let hazed = mix(p, vec3<f32>(0.9 * air), veil);
        return pow(max(hazed, vec3<f32>(0.0)), vec3<f32>(1.0 / gamma));
    }

    // The neighbourhood's dark channel: the least of five blurred taps, which keeps a
    // bright object from being read as haze just because its surroundings average bright.
    let level = 5.0;
    let t = 1.5 * exp2(level) / u.sizes.zw;
    var dark = 1e9;
    for (var i = 0; i < 5; i = i + 1) {
        var o = vec2<f32>(0.0);
        if (i == 1) { o = vec2<f32>(t.x, 0.0); }
        if (i == 2) { o = vec2<f32>(-t.x, 0.0); }
        if (i == 3) { o = vec2<f32>(0.0, t.y); }
        if (i == 4) { o = vec2<f32>(0.0, -t.y); }
        let n = exposed(scene(textureSampleLevel(sensor, sensor_sampler, uv + o, level), wb) * lens_gain);
        dark = min(dark, min(n.r, min(n.g, n.b)));
    }
    let dark_p = pow(clamp(dark, 0.0, 1.0), gamma);

    // Transmission never below 0.25: the division amplifies noise by 1/t.
    let transmission = max(1.0 - amount * 0.6 * dark_p / air, 0.25);
    let clear = (p - vec3<f32>(air)) / transmission + vec3<f32>(air);
    return pow(max(clear, vec3<f32>(0.0)), vec3<f32>(1.0 / gamma));
}

fn balance_channel(v: f32, lift: f32, gma: f32, gn: f32, off: f32) -> f32 {
    var o = v + off;
    o = o * (1.0 + gn) + lift * (1.0 - o);
    if (o > 0.0) {
        o = pow(o, 1.0 / max(1.0 + gma, 0.01));
    }
    return o;
}

fn apply_exposure_with_rolloff(c: vec3<f32>, gain: f32, black: f32, rolloff: f32, tau: f32) -> vec3<f32> {
    let scaled = max(vec3<f32>(0.0), (c - vec3<f32>(black)) * gain);
    if (rolloff <= 0.001) {
        return scaled;
    }
    let threshold = max(tau, 0.1);
    let over = max(scaled - vec3<f32>(threshold), vec3<f32>(0.0));
    let compressed = over / (vec3<f32>(1.0) + over * rolloff);
    return select(scaled, vec3<f32>(threshold) + compressed, scaled > vec3<f32>(threshold));
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let cmax = max(c.r, max(c.g, c.b));
    let cmin = min(c.r, min(c.g, c.b));
    let delta = cmax - cmin;
    var h = 0.0;
    if (delta > 0.0001) {
        if (cmax == c.r) {
            h = ((c.g - c.b) / delta) % 6.0;
        } else if (cmax == c.g) {
            h = ((c.b - c.r) / delta) + 2.0;
        } else {
            h = ((c.r - c.g) / delta) + 4.0;
        }
        h = h * 60.0;
        if (h < 0.0) {
            h = h + 360.0;
        }
    }
    let s = select(0.0, delta / max(cmax, 0.0001), cmax > 0.0001);
    let v = cmax;
    return vec3<f32>(h, s, v);
}

fn hsv_to_rgb(hsv: vec3<f32>) -> vec3<f32> {
    let h = (hsv.x % 360.0 + 360.0) % 360.0;
    let s = clamp(hsv.y, 0.0, 1.0);
    let v = max(hsv.z, 0.0);
    let c = v * s;
    let x = c * (1.0 - abs(((h / 60.0) % 2.0) - 1.0));
    let m = v - c;
    var rgb = vec3<f32>(0.0);
    if (h < 60.0) {
        rgb = vec3<f32>(c, x, 0.0);
    } else if (h < 120.0) {
        rgb = vec3<f32>(x, c, 0.0);
    } else if (h < 180.0) {
        rgb = vec3<f32>(0.0, c, x);
    } else if (h < 240.0) {
        rgb = vec3<f32>(0.0, x, c);
    } else if (h < 300.0) {
        rgb = vec3<f32>(x, 0.0, c);
    } else {
        rgb = vec3<f32>(c, 0.0, x);
    }
    return rgb + vec3<f32>(m);
}

fn apply_color_eq(rgb: vec3<f32>) -> vec3<f32> {
    var hsv = rgb_to_hsv(rgb);
    // Each hue takes its two neighbouring bands' moves, blended by where it sits between
    // their centres: a band's own hue gets exactly its move, and every hue's weights sum
    // to one, so no hue is pushed harder (or softer) than the sliders say.
    var centres = array<f32, 9>(0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 280.0, 320.0, 360.0);
    var bands = array<vec4<f32>, 9>(
        u.eq_red, u.eq_orange, u.eq_yellow, u.eq_green,
        u.eq_aqua, u.eq_blue, u.eq_purple, u.eq_magenta, u.eq_red,
    );
    let h = clamp(hsv.x, 0.0, 359.999);
    var i = 0;
    for (var k = 0; k < 8; k = k + 1) {
        if (h >= centres[k]) {
            i = k;
        }
    }
    let t = (h - centres[i]) / (centres[i + 1] - centres[i]);
    let m = mix(bands[i].xyz, bands[i + 1].xyz, t);

    hsv.x = (hsv.x + m.x + 360.0) % 360.0;
    hsv.y = clamp(hsv.y * (1.0 + m.y * 0.01), 0.0, 1.0);
    hsv.z = max(0.0, hsv.z * (1.0 + m.z * 0.01));

    return hsv_to_rgb(hsv);
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
    if (enabled(FLAG_LENS) && (u.lens.z != 1.0 || u.lens.w != 1.0)) {
        s = lens_ca(s, src, lod);
    }

    let split_pos = u.split.x;
    let is_split = split_pos >= 0.0;
    if (is_split) {
        // The divider itself is drawn by the UI, at screen resolution, so it stays one
        // line wide at every zoom instead of scaling with the image.
        let norm_x = out_px.x / u.sizes.x;
        // "Before" on the left of the split: the file as shot — as-shot WB, highlight
        // recovery, camera matrix — from the sensor as it is, so before any sharpening.
        if (norm_x < split_pos) {
            var bc = balanced(s, u.as_shot_wb.rgb);
            bc = vec3<f32>(dot(u.cam0.rgb, bc), dot(u.cam1.rgb, bc), dot(u.cam2.rgb, bc));
            return finish(bc, out_px);
        }
    }

    if (enabled(FLAG_SHARPEN)) {
        // Sharpening is a one-pixel kernel at full resolution. A reduced preview has to
        // show what that export looks like once shrunk to the screen — and a Laplacian
        // averages to almost nothing under a shrink: the Laplacian of a level-L texel is
        // 4^L times the average of the full-resolution ones under it. So it is taken at
        // this level's scale and scaled back by 4^-L, which is exact at 1:1 and faithful
        // at fit, where sharpening is correctly all but invisible.
        // All four channels: the neutral in `a` is part of the pixel's light too.
        let step = exp2(lod) / u.sizes.zw;
        let s_up = textureSampleLevel(sensor, sensor_sampler, uv - vec2<f32>(0.0, step.y), lod);
        let s_dn = textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(0.0, step.y), lod);
        let s_lf = textureSampleLevel(sensor, sensor_sampler, uv - vec2<f32>(step.x, 0.0), lod);
        let s_rt = textureSampleLevel(sensor, sensor_sampler, uv + vec2<f32>(step.x, 0.0), lod);
        let laplacian = (s * 4.0 - (s_up + s_dn + s_lf + s_rt)) * exp2(-2.0 * lod);
        s = clamp(s + laplacian * u.sharpen.x, vec4<f32>(0.0), vec4<f32>(2.0));
    }

    // After sharpening, whose kernel is a pixel across and would otherwise see the gain as
    // an edge; all four channels, since the neutral in `a` is this pixel's light too.
    if (enabled(FLAG_LENS) && u.lens_b.x != 0.0) {
        lens_gain = lens_gain_at(src);
        s = s * lens_gain;
    }

    // White balance is applied in camera space, BEFORE the primaries conversion.
    var wb = vec3<f32>(1.0);
    if (enabled(FLAG_WB)) {
        wb = u.wb.rgb;
    }
    // Camera-native primaries -> linear sRGB. Never optional: skipping this is what made
    // every image in the last build look like a broken white balance.
    var c = scene(s, wb);
    // Whether any stage produced a NaN or infinity. Checked stage by stage, not only at the
    // end, because a later `max(0, NaN)` quietly launders one into a black speck.
    var bad = not_finite(c);

    if (enabled(FLAG_CLARITY)) {
        c = c * local_contrast(s, uv, lod, wb);
        bad = bad || not_finite(c);
    }

    if (enabled(FLAG_EXPOSURE)) {
        c = apply_exposure_with_rolloff(c, u.exposure.x, u.exposure.y, u.exposure.z, u.exposure.w);
        bad = bad || not_finite(c);
    }

    if (enabled(FLAG_DEHAZE)) {
        c = dehaze(c, uv, wb);
        bad = bad || not_finite(c);
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
        bad = bad || not_finite(c);
    }

    if (enabled(FLAG_CURVE)) {
        if (u.flags.y == 1u) {
            c = vec3<f32>(curve_display(c.r), curve_display(c.g), curve_display(c.b));
        } else {
            c = vec3<f32>(curve(c.r), curve(c.g), curve(c.b));
        }
    }

    if (enabled(FLAG_FILMIC)) {
        c = filmic(c);
        bad = bad || not_finite(c);
    }

    if (enabled(FLAG_COLOR_EQ)) {
        c = apply_color_eq(c);
        bad = bad || not_finite(c);
    }

    if (enabled(FLAG_VIBRANCE)) {
        let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
        let max_ch = max(c.r, max(c.g, c.b));
        let min_ch = min(c.r, min(c.g, c.b));
        let cur_sat = (max_ch - min_ch) / max(max_ch, 0.001);
        let vib_boost = u.vibrance.x * (1.0 - clamp(cur_sat, 0.0, 1.0));
        let sat_factor = max(1.0 + u.vibrance.y + vib_boost, 0.0);
        c = mix(vec3<f32>(luma), c, sat_factor);
        bad = bad || not_finite(c);
    }

    // Colour balance grades what saturation left: after it, so a photo taken to black and
    // white can still be toned (sepia, selenium, cyanotype) instead of the tint being
    // mixed back out to grey.
    if (enabled(FLAG_BALANCE)) {
        c = vec3<f32>(
            balance_channel(c.r, u.lift.r, u.gamma.r, u.gain.r, u.offset.r),
            balance_channel(c.g, u.lift.g, u.gamma.g, u.gain.g, u.offset.g),
            balance_channel(c.b, u.lift.b, u.gamma.b, u.gain.b, u.offset.b),
        );
        bad = bad || not_finite(c);
    }

    if (enabled(FLAG_VIGNETTE)) {
        // Centred on the crop and scaled to it: the vignette frames what is exported.
        let centre = u.frame.xy + u.frame.zw * 0.5;
        let norm_pos = (out_px / u.sizes.xy - centre) / max(u.frame.zw, vec2<f32>(0.01));
        let dist = length(norm_pos) * 1.4142;
        let rad = max(u.vignette.y, 0.05);
        let v_falloff = smoothstep(rad * 0.4, rad, dist);
        c = c * max(1.0 + u.vignette.x * v_falloff, 0.0);
    }

    if (enabled(FLAG_NAN_PAINT) && (bad || not_finite(c))) {
        // Magenta to the eye, and alpha 0 to a test: every real pixel is opaque, while a
        // saturated pink can legitimately clamp to (255, 0, 255).
        return vec4<f32>(1.0, 0.0, 1.0, 0.0);
    }
    return finish(c, out_px);
}
