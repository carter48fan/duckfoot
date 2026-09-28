// The clipping overlay: the finished preview with every pixel that reached white painted
// red and every one that reached black painted blue (DESIGN.md: the convention every RAW
// editor shares). A separate pass over the preview, into its own texture, so the preview
// itself — which the histogram measures and export must equal — never carries it.

@group(0) @binding(0) var preview: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(preview, vec2<i32>(pos.xy), 0);
    // On the 8-bit values the file will hold, and as the scope's clip readout counts them:
    // every channel at 255 is detail gone to white, every one at 0 to black. (A single
    // channel at its end is a saturated colour, which still has its detail.)
    if (all(c.rgb >= vec3<f32>(254.5 / 255.0))) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    if (all(c.rgb <= vec3<f32>(0.5 / 255.0))) {
        return vec4<f32>(0.0, 0.35, 1.0, 1.0);
    }
    return c;
}
