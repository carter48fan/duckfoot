// Noise reduction: a pre-pass from the sensor into a derived texture that the adjust pass
// samples in its place (DESIGN §3.3). The sensor itself stays immutable (invariant 5), and
// the fused pass stays one pass: this runs only when the photo, the preview level or the
// Denoise settings change, not per slider frame elsewhere in the stack.
//
// One draw per mip level, each from the sensor's own level, so the preview (a coarse level)
// is denoised as what it shows rather than by shrinking a full-resolution result it never
// needs. Noise at level L is already averaged over 4^L photosites, so its spread is 2^-L of
// full resolution's, and the filter's idea of "noise" shrinks with it.
//
// Worked in camera space before white balance, as luminance plus a chroma difference:
// grain is mostly luminance, and averaged by an edge-preserving (bilateral) filter that
// leaves anything much larger than the noise alone; colour noise is coarse blotches, and
// averaged over a coarser level, weighted by luminance so colour does not bleed over edges.

struct Params {
    // x = sensor level read and written, y = luminance noise scale (0 = off),
    // z = range width in noise sigmas (detail), w = colour amount 0..1
    a: vec4<f32>,
    // x = extra levels for the colour average (smoothness), y = the sensor's last level
    b: vec4<f32>,
}

@group(0) @binding(0) var sensor: texture_2d<f32>;
@group(0) @binding(1) var<uniform> p: Params;
@group(0) @binding(2) var sensor_sampler: sampler;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// Green counts double, as it does on the mosaic.
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.25, 0.5, 0.25));
}

// The spread of shot noise plus a read-noise floor at brightness `y`, at this level.
fn sigma(y: f32, level: f32) -> f32 {
    return p.a.y * sqrt(max(y, 0.0) + 0.004) * exp2(-level);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let level = p.a.x;
    let li = i32(level);
    let size = vec2<i32>(textureDimensions(sensor, li));
    let at = vec2<i32>(pos.xy);
    let c = textureLoad(sensor, at, li);
    let y = luma(c.rgb);
    var y_out = y;

    if (p.a.y > 0.0) {
        // 5×5, spatial sigma 1.5 texels; the range kernel is `detail` noise sigmas wide,
        // so a step several times the noise — an edge, real texture — keeps its sides.
        let range = max(p.a.z * sigma(y, level), 1e-6);
        var sum = 0.0;
        var weight = 0.0;
        for (var dy = -2; dy <= 2; dy = dy + 1) {
            for (var dx = -2; dx <= 2; dx = dx + 1) {
                let q = clamp(at + vec2<i32>(dx, dy), vec2<i32>(0), size - 1);
                let yq = luma(textureLoad(sensor, q, li).rgb);
                let d2 = f32(dx * dx + dy * dy);
                let r = (yq - y) / range;
                let w = exp(-d2 / 4.5 - 0.5 * r * r);
                sum += w * yq;
                weight += w;
            }
        }
        y_out = sum / weight;
    }

    var chroma = c.rgb - vec3<f32>(y);
    if (p.a.w > 0.0) {
        // Nine taps a texel apart at a coarser level, where colour noise has already
        // averaged down, each weighted by how close its brightness is to this pixel's.
        let coarse = min(level + p.b.x, p.b.y);
        let uv = (vec2<f32>(at) + 0.5) / vec2<f32>(size);
        let step = exp2(coarse - level) / vec2<f32>(size);
        let range = max(3.0 * sigma(y, level) + 0.02 * max(y, 0.0), 1e-4);
        var sum = vec3<f32>(0.0);
        var weight = 0.0;
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let n = textureSampleLevel(
                    sensor,
                    sensor_sampler,
                    uv + vec2<f32>(f32(dx), f32(dy)) * step,
                    coarse,
                ).rgb;
                let yn = luma(n);
                let r = (yn - y) / range;
                let w = exp(-0.5 * r * r) * select(1.0, 0.5, dx != 0 || dy != 0);
                sum += w * (n - vec3<f32>(yn));
                weight += w;
            }
        }
        // The pixel's own chroma always counts, so a weight of nothing leaves it alone.
        chroma = mix(chroma, (sum + chroma * 0.25) / (weight + 0.25), p.a.w);
    }

    return vec4<f32>(vec3<f32>(y_out) + chroma, c.a);
}
