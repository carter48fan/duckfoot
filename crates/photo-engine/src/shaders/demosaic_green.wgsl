// Pass 1a: the green plane, by directional (Hamilton-Adams) interpolation.
//
// Green is done first and alone because it carries most of the luminance and is sampled at
// twice the density of red or blue. Every later channel is reconstructed relative to it, so
// an error here propagates into all three.
//
// The previous 3x3 neighbourhood average interpolated straight across edges. On fine
// periodic detail — wire shelving, guitar strings, fabric — that produces zipper artefacts
// and the magenta/green speckle that comes with them. The fix is to not average across an
// edge: estimate the gradient horizontally and vertically, and interpolate along whichever
// direction is smoother.
//
// The second-difference term (`2*c - c_left2 - c_right2`) is what makes this better than a
// directional average. The centre photosite is red or blue, and its own channel's curvature
// predicts green's curvature, because at an edge all three channels turn together. That
// correction recovers detail a plain average smooths away.
//
// Runs ONCE per image.

struct Demosaic {
    size:  vec4<u32>,   // xy = width, height
    cfa:   vec4<u32>,   // colour index at (0,0) (0,1) (1,0) (1,1)
    black: vec4<f32>,   // per CFA colour index
    white: vec4<f32>,
}

@group(0) @binding(0) var mosaic: texture_2d<u32>;
@group(0) @binding(1) var<uniform> u: Demosaic;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn colour_at(x: i32, y: i32) -> u32 {
    return u.cfa[u32(y & 1) * 2u + u32(x & 1)];
}

// Normalised against this photosite's own black/white level. No upper clamp: values above
// 1.0 are real headroom and the highlight reconstruction downstream needs to see them.
fn sample_at(x: i32, y: i32) -> f32 {
    let w = i32(u.size.x);
    let h = i32(u.size.y);
    let sx = clamp(x, 0, w - 1);
    let sy = clamp(y, 0, h - 1);
    let raw = f32(textureLoad(mosaic, vec2<i32>(sx, sy), 0).r);
    let c = colour_at(sx, sy);
    let b = u.black[c];
    return max(raw - b, 0.0) / max(u.white[c] - b, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);

    // Green photosites already measured green.
    if (colour_at(x, y) == 1u) {
        return vec4<f32>(sample_at(x, y), 0.0, 0.0, 1.0);
    }

    let centre = sample_at(x, y);

    let g_w = sample_at(x - 1, y);
    let g_e = sample_at(x + 1, y);
    let g_n = sample_at(x, y - 1);
    let g_s = sample_at(x, y + 1);

    // Two photosites out is the same colour as the centre, so these are same-channel.
    let c_w2 = sample_at(x - 2, y);
    let c_e2 = sample_at(x + 2, y);
    let c_n2 = sample_at(x, y - 2);
    let c_s2 = sample_at(x, y + 2);

    let curve_h = 2.0 * centre - c_w2 - c_e2;
    let curve_v = 2.0 * centre - c_n2 - c_s2;

    // Gradient estimate: green's own variation plus the centre channel's curvature.
    let grad_h = abs(g_w - g_e) + abs(curve_h);
    let grad_v = abs(g_n - g_s) + abs(curve_v);

    var g: f32;
    if (grad_h < grad_v) {
        g = (g_w + g_e) * 0.5 + curve_h * 0.25;
    } else if (grad_v < grad_h) {
        g = (g_n + g_s) * 0.5 + curve_v * 0.25;
    } else {
        // Genuinely isotropic — no edge to align with, so use everything.
        g = (g_w + g_e + g_n + g_s) * 0.25 + (curve_h + curve_v) * 0.125;
    }

    return vec4<f32>(max(g, 0.0), 0.0, 0.0, 1.0);
}
