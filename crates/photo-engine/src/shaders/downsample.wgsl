// Sensor mip chain: each level is a 2x2 box average of the one above it.
//
// Run once per image, right after demosaic. The preview samples the level whose texel
// spacing matches the screen, so a slider drag at fit-to-window touches a sixteenth of the
// pixels a full-resolution pass would — and averages the rest rather than skipping them,
// which is what keeps a reduced preview from aliasing.
//
// Linear camera RGB, no clamp: values above 1.0 are highlight headroom and must survive
// into every level. Alpha is the reconstructed-highlight neutral (demosaic_rcd.wgsl), and a
// box average of it is exactly right: the reconstruction was split into parts that are
// linear precisely so that this average could not undo it.

@group(0) @binding(0) var src: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    // A child level is floor(parent / 2) on each side, so an odd parent's last row or
    // column has no child of its own and is dropped; the clamp only guards the read. The
    // shift that leaves is under one texel of the child, too small to see at the zoom that
    // samples it.
    let last = vec2<i32>(textureDimensions(src)) - vec2<i32>(1, 1);
    let p = vec2<i32>(pos.xy) * 2;
    let a = textureLoad(src, min(p, last), 0);
    let b = textureLoad(src, min(p + vec2<i32>(1, 0), last), 0);
    let c = textureLoad(src, min(p + vec2<i32>(0, 1), last), 0);
    let d = textureLoad(src, min(p + vec2<i32>(1, 1), last), 0);
    return (a + b + c + d) * 0.25;
}
