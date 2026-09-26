// Pass 1b: red and blue, by colour-difference interpolation against the green plane.
//
// This is the step that removes false colour. Interpolating red and blue directly is what
// produces magenta and green speckle on fine detail: red and blue are sampled at a quarter
// of the sensor's density, so at any frequency above that they alias badly, and the aliases
// land in different places per channel.
//
// Chroma, however, varies slowly across almost every real scene, even where luminance does
// not. So instead of interpolating red, interpolate the *difference* R-G, which is nearly
// flat across the edges that wreck a direct interpolation, and add the already-reconstructed
// green back. Detail comes from green, which was sampled densely and interpolated along the
// edge rather than across it; colour comes from a signal smooth enough to survive the
// sparse sampling.
//
// Runs ONCE per image. Writes the immutable linear camera-RGB sensor texture, with the
// highlight reconstruction folded in (see `reconstruct` below).

struct Demosaic {
    size:  vec4<u32>,
    cfa:   vec4<u32>,
    black: vec4<f32>,
    white: vec4<f32>,
}

@group(0) @binding(0) var mosaic: texture_2d<u32>;
@group(0) @binding(1) var<uniform> u: Demosaic;
@group(0) @binding(2) var green_plane: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn colour_at(x: i32, y: i32) -> u32 {
    return u.cfa[u32(y & 1) * 2u + u32(x & 1)];
}

// A coordinate off the edge, mirrored about the edge photosite: an even distance from
// where it was asked for, so it keeps the pattern's colour (see demosaic_green.wgsl).
fn reflect_edge(v: i32, n: i32) -> i32 {
    var r = v;
    if (r < 0) {
        r = -r;
    }
    if (r > n - 1) {
        r = 2 * (n - 1) - r;
    }
    return clamp(r, 0, n - 1);
}

fn edge_x(x: i32) -> i32 {
    return reflect_edge(x, i32(u.size.x));
}

fn edge_y(y: i32) -> i32 {
    return reflect_edge(y, i32(u.size.y));
}

fn sample_at(x: i32, y: i32) -> f32 {
    let sx = edge_x(x);
    let sy = edge_y(y);
    let raw = f32(textureLoad(mosaic, vec2<i32>(sx, sy), 0).r);
    let c = colour_at(sx, sy);
    let b = u.black[c];
    return max(raw - b, 0.0) / max(u.white[c] - b, 1.0);
}

fn green_at(x: i32, y: i32) -> f32 {
    return textureLoad(green_plane, vec2<i32>(edge_x(x), edge_y(y)), 0).r;
}

/// The colour difference `sample - green` at a neighbour, which is the quantity that
/// interpolates cleanly.
fn diff_at(x: i32, y: i32) -> f32 {
    return sample_at(x, y) - green_at(x, y);
}

/// Mean colour difference over the four diagonal neighbours. Used at a red site to find
/// blue and vice versa — the opposite colour sits diagonally in every 2x2 Bayer layout.
fn diagonal_diff(x: i32, y: i32) -> f32 {
    return (diff_at(x - 1, y - 1)
        + diff_at(x + 1, y - 1)
        + diff_at(x - 1, y + 1)
        + diff_at(x + 1, y + 1))
        * 0.25;
}

// Where a photosite stops carrying colour information. Demosaicing averages neighbours,
// so a clipped site lands slightly under 1.0; the ramp starts early enough to catch that
// without desaturating legitimately bright colour.
const CLIP_LO: f32 = 0.93;
const CLIP_HI: f32 = 1.0;

/// The brightest photosite in the 3x3 block around a pixel, every colour.
///
/// A pixel next to a clipped site is built partly from it — green is interpolated across
/// it, red and blue take colour differences through it — so its channels are wrong by
/// unequal amounts even though its own photosite measured correctly. Judged only on its own
/// demosaiced value it can sit well under the ramp, and it is the one-pixel magenta rim
/// round every blown specular.
fn raw_peak(x: i32, y: i32) -> f32 {
    var peak = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            peak = max(peak, sample_at(x + dx, y + dy));
        }
    }
    return peak;
}

// Highlight reconstruction.
//
// A photosite saturates at 1.0 after black/white normalisation, so a blown specular reads
// (1,1,1) there no matter what colour it actually was. White balance then multiplies those
// three equal values by three unequal gains — [2.03, 1.00, 1.80] on a Sony A7 III — and a
// neutral highlight comes out magenta. That is not a white balance error; it is a channel
// being asked to report a value it never measured.
//
// A saturated photosite carries no colour, so the only defensible colour is neutral, at
// green's level. On a neutral subject green is the first channel to saturate (its balance
// gain is 1.0, red's and blue's are above it), so at the clip green IS the clip level. And
// at the rim of a blown area, where red and blue come from colour differences taken
// through clipped sites and can undershoot all the way to zero, green was interpolated
// along the edge and is the channel still worth trusting. Collapsing to the minimum
// channel instead turns those rim pixels black.
//
// It is done here, once, rather than in the adjust pass, because it is not linear and the
// adjust pass never sees full-resolution pixels at preview zoom: it samples a box-filtered
// mip, and a mip texel that is half blown and half not has a peak nowhere near the clip, so
// it came through magenta. Instead the result is stored as two parts that ARE linear —
// `rgb`, the camera colour still trusted, and `a`, the neutral replacing the rest — so every
// mip level and every filtered sample carries the reconstruction with it. The adjust pass
// recombines them as `rgb * wb + a`: white balance scales the measured colour and leaves
// the neutral alone. Green's balance gain is always 1.0, so the neutral needs no balance
// of its own and stays neutral under any white balance the user picks.
//
// This reconstructs colour, not detail. Texture inside a blown highlight is gone at capture
// and no amount of arithmetic here brings it back.
fn reconstruct(cam: vec3<f32>, x: i32, y: i32) -> vec4<f32> {
    let peak = max(max(cam.r, max(cam.g, cam.b)), raw_peak(x, y));
    let t = smoothstep(CLIP_LO, CLIP_HI, peak);
    return vec4<f32>(cam * (1.0 - t), cam.g * t);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);

    let g = green_at(x, y);
    let here = colour_at(x, y);

    var r: f32;
    var b: f32;

    if (here == 1u) {
        // A green photosite: one axis carries red, the other blue. Which is which depends
        // on the CFA phase, so ask rather than assume — this is where a hardcoded RGGB
        // assumption swaps the channels on a BGGR or GRBG sensor.
        let horizontal = (diff_at(x - 1, y) + diff_at(x + 1, y)) * 0.5;
        let vertical = (diff_at(x, y - 1) + diff_at(x, y + 1)) * 0.5;
        if (colour_at(x - 1, y) == 0u) {
            r = g + horizontal;
            b = g + vertical;
        } else {
            r = g + vertical;
            b = g + horizontal;
        }
    } else if (here == 0u) {
        // Red photosite: red is measured, blue comes from the diagonals.
        r = sample_at(x, y);
        b = g + diagonal_diff(x, y);
    } else {
        b = sample_at(x, y);
        r = g + diagonal_diff(x, y);
    }

    // A colour difference can push a channel below zero where green was over-estimated at
    // a hard edge. Negative light is not meaningful and would survive into the matrix.
    return reconstruct(vec3<f32>(max(r, 0.0), g, max(b, 0.0)), x, y);
}
