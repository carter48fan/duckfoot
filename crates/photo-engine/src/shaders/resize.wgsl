// GPU-accelerated export resizing: downsamples from the cropped source region
// directly into the destination dimensions using bilinear hardware interpolation.

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var s_sampler: sampler;

struct ResizeUniforms {
    // xy = crop min (0..1 of src), zw = crop span (0..1 of src)
    crop: vec4<f32>,
    // xy = target width/height (px)
    dest_size: vec4<f32>,
}
@group(0) @binding(2) var<uniform> u: ResizeUniforms;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv_dest = pos.xy / u.dest_size.xy;
    let uv_src = u.crop.xy + uv_dest * u.crop.zw;
    return textureSampleLevel(src_tex, s_sampler, uv_src, 0.0);
}
