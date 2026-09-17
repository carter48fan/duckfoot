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
// Runs ONCE per image. Writes the immutable linear camera-RGB sensor texture.

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

fn clamp_x(x: i32) -> i32 {
    return clamp(x, 0, i32(u.size.x) - 1);
}

fn clamp_y(y: i32) -> i32 {
    return clamp(y, 0, i32(u.size.y) - 1);
}

fn sample_at(x: i32, y: i32) -> f32 {
    let sx = clamp_x(x);
    let sy = clamp_y(y);
    let raw = f32(textureLoad(mosaic, vec2<i32>(sx, sy), 0).r);
    let c = colour_at(sx, sy);
    let b = u.black[c];
    return max(raw - b, 0.0) / max(u.white[c] - b, 1.0);
}

fn green_at(x: i32, y: i32) -> f32 {
    return textureLoad(green_plane, vec2<i32>(clamp_x(x), clamp_y(y)), 0).r;
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
    return vec4<f32>(max(r, 0.0), g, max(b, 0.0), 1.0);
}
