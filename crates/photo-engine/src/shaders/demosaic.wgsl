// Pass 1: CFA mosaic -> linear camera-native RGB.
//
// Runs ONCE per image. The texture it writes is immutable for the lifetime of the image
// (DESIGN.md invariant 5) — every adjustment reads it and nothing writes to it again.
//
// The demosaic is a 3x3 neighbourhood average, general over any 2x2 CFA pattern. That is
// deliberately modest: it is correct for every Bayer layout, which is what the last build
// got wrong by hardcoding RGGB. A better kernel (RCD/AHD) is a drop-in replacement here
// and changes nothing downstream.

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
    // Oversized triangle covering the viewport; no vertex buffer.
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn colour_at(x: i32, y: i32) -> u32 {
    let cx = u32(x & 1);
    let cy = u32(y & 1);
    return u.cfa[cy * 2u + cx];
}

fn sample_at(x: i32, y: i32) -> f32 {
    let w = i32(u.size.x);
    let h = i32(u.size.y);
    let sx = clamp(x, 0, w - 1);
    let sy = clamp(y, 0, h - 1);
    let raw = f32(textureLoad(mosaic, vec2<i32>(sx, sy), 0).r);

    // Normalise to 0..1 against this photosite's own black/white level. Using channel 0
    // for every channel, as the last build did, tints anything with per-channel levels.
    let c = colour_at(sx, sy);
    let b = u.black[c];
    let range = max(u.white[c] - b, 1.0);
    return max(raw - b, 0.0) / range;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);

    var sum = vec3<f32>(0.0);
    var count = vec3<f32>(0.0);

    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let c = colour_at(x + dx, y + dy);
            sum[c] = sum[c] + sample_at(x + dx, y + dy);
            count[c] = count[c] + 1.0;
        }
    }

    var rgb = sum / max(count, vec3<f32>(1.0));

    // This photosite measured its own colour directly — do not average that one away.
    let own = colour_at(x, y);
    rgb[own] = sample_at(x, y);

    return vec4<f32>(rgb, 1.0);
}
