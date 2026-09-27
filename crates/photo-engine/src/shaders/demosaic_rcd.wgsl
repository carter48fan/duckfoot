// Bayer demosaic by RCD, Ratio Corrected Demosaicing (Luis Sanz Rodríguez, 2017; as in
// RawTherapee and darktable), in four passes.
//
// Hamilton-Adams (which this replaces) chose one direction per pixel from two gradient
// sums and interpolated along it: at a slanted or curved edge it flipped between the two
// from pixel to pixel, which is the zipper and the maze on fine detail. RCD instead
//
//  1. measures how strongly the mosaic varies vertically and horizontally (and along the
//     two diagonals) with a high-pass over each line, smoothed over three lines, and keeps
//     the ratio V / (V + H) as a *soft* direction, 0.5 where there is none;
//  2. estimates green at a red or blue site from each of its four neighbours, each scaled
//     by the ratio of a low-pass of the mosaic here and two sites away in that direction
//     (colour ratios vary more slowly than colour differences across an edge), weights
//     each pair by the gradient on its far side, and blends vertical and horizontal by
//     the soft direction, taken from the neighbours when they are surer;
//  3. fills red at blue sites and blue at red ones from diagonal colour differences, the
//     same way along the two diagonals;
//  4. fills red and blue at green sites from the cardinal colour differences.
//
// Runs ONCE per image. The last pass writes the immutable linear camera-RGB sensor texture,
// with the highlight reconstruction folded in (see `reconstruct` below).

struct Demosaic {
    size:  vec4<u32>,              // xy = width, height; zw = pattern size, 2×2 here
    cfa:   array<vec4<u32>, 9>,    // colour index per pattern position, row-major
    black: vec4<f32>,              // per CFA colour index
    white: vec4<f32>,
}

@group(0) @binding(0) var mosaic: texture_2d<u32>;
@group(0) @binding(1) var<uniform> u: Demosaic;
// Pass 1's output: x = V / (V + H), y = P / (P + Q) (P the NW-SE diagonal).
@group(0) @binding(2) var dirs: texture_2d<f32>;
// Pass 2's output.
@group(0) @binding(3) var green_plane: texture_2d<f32>;
// Pass 3's output: blue at red sites, red at blue ones.
@group(0) @binding(4) var opposite: texture_2d<f32>;
// The trusted colour ratios, with their mips (see `fs_ratios`).
@group(0) @binding(5) var ratios: texture_2d<f32>;
@group(0) @binding(6) var ratio_sampler: sampler;

const EPS: f32 = 1e-5;
const EPSSQ: f32 = 1e-10;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn colour_at(x: i32, y: i32) -> u32 {
    let i = u32(y & 1) * 2u + u32(x & 1);
    return u.cfa[0][i];
}

// A coordinate off the edge, mirrored about the edge photosite: -1 reads 1, w reads w - 2.
// Always an even distance from where it was asked for, so it lands on the same colour of
// the pattern; clamping to the edge instead read the neighbouring colour, and the outer
// pixel or two came out tinted.
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

fn at(x: i32, y: i32) -> vec2<i32> {
    return vec2<i32>(reflect_edge(x, i32(u.size.x)), reflect_edge(y, i32(u.size.y)));
}

// The mosaic, against its photosite's own black and white. Values above 1.0 are headroom
// the highlight reconstruction needs to see.
fn cfa(x: i32, y: i32) -> f32 {
    let p = at(x, y);
    let raw = f32(textureLoad(mosaic, p, 0).r);
    let c = colour_at(p.x, p.y);
    let b = u.black[c];
    return max(raw - b, 0.0) / max(u.white[c] - b, 1.0);
}

fn green(x: i32, y: i32) -> f32 {
    return textureLoad(green_plane, at(x, y), 0).r;
}

fn dir(x: i32, y: i32) -> vec2<f32> {
    return textureLoad(dirs, at(x, y), 0).xy;
}

/// Colour `c` (0 red, 2 blue) at a red or blue site: measured, or pass 3's.
fn rb(c: u32, x: i32, y: i32) -> f32 {
    let p = at(x, y);
    if (colour_at(p.x, p.y) == c) {
        return cfa(x, y);
    }
    return textureLoad(opposite, p, 0).r;
}

fn sqr(v: f32) -> f32 {
    return v * v;
}

// The squared high-pass along a line through (x, y), stepping (sx, sy).
fn high_pass(x: i32, y: i32, sx: i32, sy: i32) -> f32 {
    let t = cfa(x - 3 * sx, y - 3 * sy) - cfa(x - sx, y - sy) - cfa(x + sx, y + sy)
        + cfa(x + 3 * sx, y + 3 * sy);
    return sqr(t - 3.0 * (cfa(x - 2 * sx, y - 2 * sy) + cfa(x + 2 * sx, y + 2 * sy))
        + 6.0 * cfa(x, y));
}

// How strongly the mosaic varies along (sx, sy), over three neighbouring lines.
fn strength(x: i32, y: i32, sx: i32, sy: i32) -> f32 {
    return max(EPSSQ, high_pass(x - sx, y - sy, sx, sy) + high_pass(x, y, sx, sy)
        + high_pass(x + sx, y + sy, sx, sy));
}

@fragment
fn fs_dirs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let v = strength(x, y, 0, 1);
    let h = strength(x, y, 1, 0);
    // Diagonals only matter at red and blue sites, where pass 3 reads them.
    var pq = 0.5;
    if (colour_at(x, y) != 1u) {
        let p = strength(x, y, 1, 1);
        let q = strength(x, y, -1, 1);
        pq = p / (p + q);
    }
    return vec4<f32>(v / (v + h), pq, 0.0, 1.0);
}

// The surer of a pixel's own direction and its diagonal neighbours' mean.
fn refined(x: i32, y: i32, which: u32) -> f32 {
    let centre = dir(x, y)[which];
    let around = 0.25 * (dir(x - 1, y - 1)[which] + dir(x + 1, y - 1)[which]
        + dir(x - 1, y + 1)[which] + dir(x + 1, y + 1)[which]);
    if (abs(0.5 - centre) < abs(0.5 - around)) {
        return around;
    }
    return centre;
}

// A low-pass of the mosaic around a red or blue site: its own sample, its four green
// neighbours and its four diagonal opposite-colour ones.
fn low_pass(x: i32, y: i32) -> f32 {
    return cfa(x, y)
        + 0.5 * (cfa(x, y - 1) + cfa(x, y + 1) + cfa(x - 1, y) + cfa(x + 1, y))
        + 0.25 * (cfa(x - 1, y - 1) + cfa(x + 1, y - 1) + cfa(x - 1, y + 1)
            + cfa(x + 1, y + 1));
}

@fragment
fn fs_green(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    if (colour_at(x, y) == 1u) {
        return vec4<f32>(cfa(x, y), 0.0, 0.0, 1.0);
    }
    let c = cfa(x, y);

    // The gradient on each side.
    let n_grad = EPS + abs(cfa(x, y - 1) - cfa(x, y + 1)) + abs(c - cfa(x, y - 2))
        + abs(cfa(x, y - 1) - cfa(x, y - 3)) + abs(cfa(x, y - 2) - cfa(x, y - 4));
    let s_grad = EPS + abs(cfa(x, y - 1) - cfa(x, y + 1)) + abs(c - cfa(x, y + 2))
        + abs(cfa(x, y + 1) - cfa(x, y + 3)) + abs(cfa(x, y + 2) - cfa(x, y + 4));
    let w_grad = EPS + abs(cfa(x - 1, y) - cfa(x + 1, y)) + abs(c - cfa(x - 2, y))
        + abs(cfa(x - 1, y) - cfa(x - 3, y)) + abs(cfa(x - 2, y) - cfa(x - 4, y));
    let e_grad = EPS + abs(cfa(x - 1, y) - cfa(x + 1, y)) + abs(c - cfa(x + 2, y))
        + abs(cfa(x + 1, y) - cfa(x + 3, y)) + abs(cfa(x + 2, y) - cfa(x + 4, y));

    // Each neighbour's green, corrected by the low-pass ratio towards it.
    let lp = low_pass(x, y);
    let n_est = cfa(x, y - 1) * 2.0 * lp / (EPS + lp + low_pass(x, y - 2));
    let s_est = cfa(x, y + 1) * 2.0 * lp / (EPS + lp + low_pass(x, y + 2));
    let w_est = cfa(x - 1, y) * 2.0 * lp / (EPS + lp + low_pass(x - 2, y));
    let e_est = cfa(x + 1, y) * 2.0 * lp / (EPS + lp + low_pass(x + 2, y));

    let v_est = (s_grad * n_est + n_grad * s_est) / (n_grad + s_grad);
    let h_est = (w_grad * e_est + e_grad * w_est) / (e_grad + w_grad);

    let vh = refined(x, y, 0u);
    return vec4<f32>(max(mix(v_est, h_est, vh), 0.0), 0.0, 0.0, 1.0);
}

@fragment
fn fs_opposite(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let here = colour_at(x, y);
    if (here == 1u) {
        return vec4<f32>(0.0);
    }
    // The opposite colour sits on the diagonals, measured, at odd steps.
    let g = green(x, y);
    let nw_grad = EPS + abs(cfa(x - 1, y - 1) - cfa(x + 1, y + 1))
        + abs(cfa(x - 1, y - 1) - cfa(x - 3, y - 3)) + abs(g - green(x - 2, y - 2));
    let ne_grad = EPS + abs(cfa(x + 1, y - 1) - cfa(x - 1, y + 1))
        + abs(cfa(x + 1, y - 1) - cfa(x + 3, y - 3)) + abs(g - green(x + 2, y - 2));
    let sw_grad = EPS + abs(cfa(x + 1, y - 1) - cfa(x - 1, y + 1))
        + abs(cfa(x - 1, y + 1) - cfa(x - 3, y + 3)) + abs(g - green(x - 2, y + 2));
    let se_grad = EPS + abs(cfa(x - 1, y - 1) - cfa(x + 1, y + 1))
        + abs(cfa(x + 1, y + 1) - cfa(x + 3, y + 3)) + abs(g - green(x + 2, y + 2));

    let nw_est = cfa(x - 1, y - 1) - green(x - 1, y - 1);
    let ne_est = cfa(x + 1, y - 1) - green(x + 1, y - 1);
    let sw_est = cfa(x - 1, y + 1) - green(x - 1, y + 1);
    let se_est = cfa(x + 1, y + 1) - green(x + 1, y + 1);

    let p_est = (nw_grad * se_est + se_grad * nw_est) / (nw_grad + se_grad);
    let q_est = (ne_grad * sw_est + sw_grad * ne_est) / (ne_grad + sw_grad);

    let pq = refined(x, y, 1u);
    // pq is P's share of the variation: much along P, so trust Q.
    return vec4<f32>(max(g + mix(p_est, q_est, pq), 0.0), 0.0, 0.0, 1.0);
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
// That neutral is now only for what cannot be rebuilt: where some channels survived, the
// clipped ones are rebuilt from them first (Highlight detail, below), and only where every
// channel clipped, or nothing unclipped is near, is the colour truly gone at capture.
// Highlight detail.
//
// Where only some of a pixel's channels clipped (a sunset's red, foliage's green), the
// ones that did not still hold its texture, and the clipped ones can be rebuilt from them
// by the colour ratio of what is nearby and unclipped: a sky is much the same colour just
// below where it blew. The ratios come from the mosaic itself, one full sample per 2×2
// Bayer quad (so they need no demosaic, and cost a sixteenth of the sensor's memory),
// weighted to well-exposed quads with nothing clipped, then box-averaged down a mip chain
// so that a pixel deep in a large blown area still finds some.

// Beyond this a ratio is noise in a dark channel, not a colour.
const RATIO_MAX: f32 = 16.0;

/// One quad's ratios red / green and blue / green, times how far they are to be trusted,
/// and that trust.
@fragment
fn fs_ratios(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x) * 2;
    let y = i32(pos.y) * 2;
    var rgb = vec3<f32>(0.0);
    var peak = 0.0;
    for (var dy = 0; dy < 2; dy = dy + 1) {
        for (var dx = 0; dx < 2; dx = dx + 1) {
            let v = cfa(x + dx, y + dy);
            let c = colour_at(x + dx, y + dy);
            // Two greens to a quad: each is half of it.
            rgb[c] = rgb[c] + select(v, 0.5 * v, c == 1u);
            peak = max(peak, v);
        }
    }
    let trust = smoothstep(0.02, 0.25, rgb.g) * (1.0 - smoothstep(0.8, CLIP_LO, peak));
    if (trust <= 0.0) {
        return vec4<f32>(0.0);
    }
    let ratio = min(rgb.rb / rgb.g, vec2<f32>(RATIO_MAX));
    return vec4<f32>(ratio * trust, trust, 0.0);
}

/// The trusted ratios (red / green, blue / green) around `(x, y)`, and how much of them
/// was found (0 when nothing nearby is unclipped). The finest level with enough trusted
/// quads is used, blended into coarser ones where it has too few, so the ratio field is
/// smooth where the levels hand over.
fn trusted_ratio(x: i32, y: i32) -> vec3<f32> {
    let uv = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(u.size.xy);
    var remaining = 1.0;
    var acc = vec2<f32>(0.0);
    let levels = i32(textureNumLevels(ratios));
    for (var level = 0; level < levels; level = level + 1) {
        let s = textureSampleLevel(ratios, ratio_sampler, uv, f32(level));
        if (s.z > 1e-4) {
            let share = remaining * min(s.z / 0.25, 1.0);
            acc = acc + share * s.xy / s.z;
            remaining = remaining - share;
            if (remaining < 0.01) {
                break;
            }
        }
    }
    let found = 1.0 - remaining;
    return vec3<f32>(acc / max(found, 1e-6), found);
}

/// The brightest photosite of each colour in the 3×3 block around a pixel.
///
/// A pixel next to a clipped site is built partly from it (green interpolated across it,
/// red and blue taken through it), so a channel is only as good as the sites of its colour
/// around it: judged on its own demosaiced value it can sit well under the ramp, and that
/// is the one-pixel magenta rim round every blown specular.
fn raw_peaks(x: i32, y: i32) -> vec3<f32> {
    var peak = vec3<f32>(0.0);
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let c = colour_at(x + dx, y + dy);
            peak[c] = max(peak[c], cfa(x + dx, y + dy));
        }
    }
    return peak;
}

/// A pixel's camera colour, the channels that clipped rebuilt from the least clipped one
/// by the nearby ratios, then split as `rgb` still trusted and `a` neutral for what could
/// not be rebuilt: every channel clipped, or no unclipped colour anywhere near.
fn reconstruct(cam: vec3<f32>, x: i32, y: i32) -> vec4<f32> {
    let clip = smoothstep(vec3<f32>(CLIP_LO), vec3<f32>(CLIP_HI), max(cam, raw_peaks(x, y)));
    let worst = max(clip.r, max(clip.g, clip.b));
    if (worst <= 0.0) {
        return vec4<f32>(cam, 0.0);
    }
    // The least clipped channel, green on a tie: it is the densest.
    var k = 1u;
    if (clip.r < clip[k]) {
        k = 0u;
    }
    if (clip.b < clip[k]) {
        k = 2u;
    }
    let found = trusted_ratio(x, y);
    let ratio = vec3<f32>(found.x, 1.0, found.y);
    // What each channel would read at that colour, given the reference channel's reading.
    // A channel at the clip truly read at least what it did; one only beside clipped sites
    // is raised by them (the demosaic reached across), and its own reading is no floor.
    let estimate = ratio * (cam[k] / max(ratio[k], 1e-4));
    let floor = cam * step(vec3<f32>(CLIP_LO), cam);
    let rebuilt = mix(cam, max(estimate, floor), clip * found.z);
    let t = worst * max(clip[k], 1.0 - found.z);
    return vec4<f32>(rebuilt * (1.0 - t), cam.g * t);
}

@fragment
fn fs_final(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let g = green(x, y);
    var r: f32;
    var b: f32;
    if (colour_at(x, y) == 1u) {
        let vh = refined(x, y, 0u);
        var out = array<f32, 2>(0.0, 0.0);
        for (var k = 0u; k < 2u; k = k + 1u) {
            let c = k * 2u;
            let n_grad = EPS + abs(g - green(x, y - 2)) + abs(rb(c, x, y - 1) - rb(c, x, y + 1))
                + abs(rb(c, x, y - 1) - rb(c, x, y - 3));
            let s_grad = EPS + abs(g - green(x, y + 2)) + abs(rb(c, x, y + 1) - rb(c, x, y - 1))
                + abs(rb(c, x, y + 1) - rb(c, x, y + 3));
            let w_grad = EPS + abs(g - green(x - 2, y)) + abs(rb(c, x - 1, y) - rb(c, x + 1, y))
                + abs(rb(c, x - 1, y) - rb(c, x - 3, y));
            let e_grad = EPS + abs(g - green(x + 2, y)) + abs(rb(c, x + 1, y) - rb(c, x - 1, y))
                + abs(rb(c, x + 1, y) - rb(c, x + 3, y));
            let n_est = rb(c, x, y - 1) - green(x, y - 1);
            let s_est = rb(c, x, y + 1) - green(x, y + 1);
            let w_est = rb(c, x - 1, y) - green(x - 1, y);
            let e_est = rb(c, x + 1, y) - green(x + 1, y);
            let v_est = (n_grad * s_est + s_grad * n_est) / (n_grad + s_grad);
            let h_est = (e_grad * w_est + w_grad * e_est) / (e_grad + w_grad);
            out[k] = max(g + mix(v_est, h_est, vh), 0.0);
        }
        r = out[0];
        b = out[1];
    } else if (colour_at(x, y) == 0u) {
        r = cfa(x, y);
        b = textureLoad(opposite, at(x, y), 0).r;
    } else {
        b = cfa(x, y);
        r = textureLoad(opposite, at(x, y), 0).r;
    }
    return reconstruct(vec3<f32>(r, g, b), x, y);
}
