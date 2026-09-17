// Scopes: a 256-bin RGB + luminance histogram, computed on the GPU.
//
// DESIGN.md invariant 4 allows exactly one readback in the interactive path, and this is
// it: 4 KB of bins, never a frame. Reading the image back to count pixels on the CPU would
// be 92 MB per change, which is the webview mistake wearing a lab coat.
//
// The source is the adjusted output texture, which is display-referred — so the histogram
// describes what is actually on screen, which is the only thing a photographer can act on.
//
// Sampling is strided rather than exhaustive. 256 bins accumulated from ~1.5M samples is
// already far past the point where more samples change the shape, and 24M atomic
// increments into 256 addresses is pure contention.

struct Params {
    region: vec4<u32>, // xy = origin, zw = size of the measured region
    stride: vec4<u32>, // x = sample every Nth pixel on both axes
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;
@group(0) @binding(2) var<uniform> u: Params;

const RED: u32 = 0u;
const GREEN: u32 = 256u;
const BLUE: u32 = 512u;
const LUMA: u32 = 768u;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    // Measured over the crop, not the whole frame: scopes describe the photograph the
    // user is framing, and counting pixels they have cropped out would mislead.
    let x = u.region.x + gid.x * u.stride.x;
    let y = u.region.y + gid.y * u.stride.x;
    if (gid.x * u.stride.x >= u.region.z || gid.y * u.stride.x >= u.region.w) {
        return;
    }

    let c = textureLoad(source, vec2<i32>(i32(x), i32(y)), 0).rgb;

    let r = u32(clamp(c.r, 0.0, 1.0) * 255.0 + 0.5);
    let g = u32(clamp(c.g, 0.0, 1.0) * 255.0 + 0.5);
    let b = u32(clamp(c.b, 0.0, 1.0) * 255.0 + 0.5);
    // Rec.709 luma. The source is already display-encoded, so this is the perceptual
    // brightness of the pixel as shown, not scene luminance.
    let l = u32(clamp(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0) * 255.0 + 0.5);

    atomicAdd(&bins[RED + r], 1u);
    atomicAdd(&bins[GREEN + g], 1u);
    atomicAdd(&bins[BLUE + b], 1u);
    atomicAdd(&bins[LUMA + l], 1u);
}
