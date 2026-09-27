// Pass 1, for Fuji's X-Trans: green, then red and blue against it, as for Bayer
// (demosaic_green.wgsl, demosaic_rb.wgsl), on a 6×6 pattern.
//
// X-Trans puts green on 20 of 36 photosites and gives every row and column all three
// colours, so its greens are dense but irregular, and red and blue never sit at the fixed
// offsets the Bayer shaders assume. Both passes here look the pattern up instead of
// assuming it:
//
// - `fs_green`: at a red or blue site, green is estimated along each of the four
//   directions from the nearest green on either side, and the estimates are blended by
//   how smooth each direction is there — the directional choice Markesteijn's method
//   makes, without its multi-pass homogeneity maps. Along an edge the estimate that
//   crosses it is steep and weighs next to nothing.
// - `fs_rb`: red and blue by colour difference against green, averaged over the red or
//   blue sites in the 5×5 around the pixel — every 5×5 on X-Trans holds several of each —
//   weighted by nearness and by how alike green is there, so colour does not bleed across
//   an edge green can see.
//
// Highlight reconstruction is the Bayer path's (see `reconstruct` in demosaic_rb.wgsl).
//
// Runs ONCE per image.

struct Demosaic {
    size:  vec4<u32>,              // xy = width, height; zw = pattern width, height
    cfa:   array<vec4<u32>, 9>,    // colour index per pattern position, row-major
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

fn inside(p: vec2<i32>) -> vec2<i32> {
    return clamp(p, vec2<i32>(0), vec2<i32>(i32(u.size.x) - 1, i32(u.size.y) - 1));
}

fn colour_at(p: vec2<i32>) -> u32 {
    let pw = i32(u.size.z);
    let ph = i32(u.size.w);
    let i = u32((((p.y % ph) + ph) % ph) * pw + ((p.x % pw) + pw) % pw);
    return u.cfa[i / 4u][i % 4u];
}

// Normalised against the photosite's own black and white. No upper clamp: headroom above
// 1.0 is what highlight reconstruction reads.
fn sample_at(p: vec2<i32>) -> f32 {
    let q = inside(p);
    let raw = f32(textureLoad(mosaic, q, 0).r);
    let c = colour_at(q);
    let b = u.black[c];
    return max(raw - b, 0.0) / max(u.white[c] - b, 1.0);
}

@fragment
fn fs_green(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    if (colour_at(p) == 1u) {
        return vec4<f32>(sample_at(p), 0.0, 0.0, 1.0);
    }

    var dirs = array<vec2<i32>, 4>(
        vec2<i32>(1, 0),
        vec2<i32>(0, 1),
        vec2<i32>(1, 1),
        vec2<i32>(1, -1),
    );
    var sum = 0.0;
    var weight = 0.0;
    for (var d = 0; d < 4; d = d + 1) {
        let dir = dirs[d];
        // The nearest green each way, up to three photosites out. Clamped at the frame's
        // edge, where the pattern still says which colour the clamped site measured.
        var ka = 0;
        var kb = 0;
        var ga = 0.0;
        var gb = 0.0;
        for (var k = 1; k <= 3; k = k + 1) {
            if (ka == 0 && colour_at(inside(p + dir * k)) == 1u) {
                ka = k;
                ga = sample_at(p + dir * k);
            }
            if (kb == 0 && colour_at(inside(p - dir * k)) == 1u) {
                kb = k;
                gb = sample_at(p - dir * k);
            }
        }
        if (ka == 0 || kb == 0) {
            continue;
        }
        let fa = f32(ka);
        let fb = f32(kb);
        let estimate = (ga * fb + gb * fa) / (fa + fb);
        // Steepness per photosite; diagonals are √2 longer per step.
        let reach = (fa + fb) * select(1.0, 1.4142, dir.x != 0 && dir.y != 0);
        let grad = abs(ga - gb) / reach;
        let w = 1.0 / ((grad + 1e-3) * (grad + 1e-3) * reach);
        sum += w * estimate;
        weight += w;
    }
    var g = sample_at(p);
    if (weight > 0.0) {
        g = sum / weight;
    }
    return vec4<f32>(max(g, 0.0), 0.0, 0.0, 1.0);
}

fn green_at(p: vec2<i32>) -> f32 {
    return textureLoad(green_plane, inside(p), 0).r;
}

// Red (0) or blue (2) at `p`, from the colour differences of the sites of that colour in
// the 5×5 around it.
fn by_difference(p: vec2<i32>, colour: u32, g: f32) -> f32 {
    var sum = 0.0;
    var weight = 0.0;
    for (var dy = -2; dy <= 2; dy = dy + 1) {
        for (var dx = -2; dx <= 2; dx = dx + 1) {
            let q = inside(p + vec2<i32>(dx, dy));
            if ((dx == 0 && dy == 0) || colour_at(q) != colour) {
                continue;
            }
            let gq = green_at(q);
            let near = 1.0 / f32(dx * dx + dy * dy);
            let alike = 1.0 / (abs(gq - g) + 0.01);
            let w = near * alike;
            sum += w * (sample_at(q) - gq);
            weight += w;
        }
    }
    if (weight == 0.0) {
        return g;
    }
    return g + sum / weight;
}

const CLIP_LO: f32 = 0.93;
const CLIP_HI: f32 = 1.0;

fn raw_peak(p: vec2<i32>) -> f32 {
    var peak = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            peak = max(peak, sample_at(p + vec2<i32>(dx, dy)));
        }
    }
    return peak;
}

// As demosaic_rb.wgsl's: a saturated photosite carries no colour, so it becomes a neutral
// at green's level, stored in `a` so every mip and filtered sample carries it.
fn reconstruct(cam: vec3<f32>, p: vec2<i32>) -> vec4<f32> {
    let peak = max(max(cam.r, max(cam.g, cam.b)), raw_peak(p));
    let t = smoothstep(CLIP_LO, CLIP_HI, peak);
    return vec4<f32>(cam * (1.0 - t), cam.g * t);
}

@fragment
fn fs_rb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let g = green_at(p);
    let here = colour_at(p);
    var r: f32;
    var b: f32;
    if (here == 0u) {
        r = sample_at(p);
    } else {
        r = by_difference(p, 0u, g);
    }
    if (here == 2u) {
        b = sample_at(p);
    } else {
        b = by_difference(p, 2u, g);
    }
    return reconstruct(vec3<f32>(max(r, 0.0), g, max(b, 0.0)), p);
}
