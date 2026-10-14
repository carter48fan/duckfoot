//! The GPU side: upload once, demosaic once, adjust per frame.
//!
//! `Engine` never creates a `Device` and never touches a window. It is handed both by
//! whatever shell is running (eframe on desktop, `android-activity` later), which is what
//! makes an Android build a new bootstrap rather than a second implementation.

use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use wgpu::util::DeviceExt;

/// How many bind groups, textures and buffers the engine has made, ever. Every one goes
/// through [`Make`], so a test can hold the engine to invariant 6: a slider drag makes
/// none (tests/drag.rs).
static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[doc(hidden)]
pub fn gpu_objects_made() -> u64 {
    MADE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The engine's way to make a GPU object: `create_*`, counted.
trait Make {
    fn make_bind_group(&self, desc: &wgpu::BindGroupDescriptor) -> wgpu::BindGroup;
    fn make_texture(&self, desc: &wgpu::TextureDescriptor) -> wgpu::Texture;
    fn make_buffer(&self, desc: &wgpu::BufferDescriptor) -> wgpu::Buffer;
    fn make_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor) -> wgpu::Buffer;
}

fn made() {
    MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

impl Make for wgpu::Device {
    fn make_bind_group(&self, desc: &wgpu::BindGroupDescriptor) -> wgpu::BindGroup {
        made();
        self.create_bind_group(desc)
    }
    fn make_texture(&self, desc: &wgpu::TextureDescriptor) -> wgpu::Texture {
        made();
        self.create_texture(desc)
    }
    fn make_buffer(&self, desc: &wgpu::BufferDescriptor) -> wgpu::Buffer {
        made();
        self.create_buffer(desc)
    }
    fn make_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor) -> wgpu::Buffer {
        made();
        self.create_buffer_init(desc)
    }
}

use crate::curve::{build_lut_4channel, CurveNode, LUT_SIZE};
use crate::decode::{CfaImage, CfaPattern, RasterImage};
use crate::export::ExportedImage;
use crate::histogram::{Histogram, ReadbackState, BUFFER_BYTES};
use crate::lens::{FileWarp, LensModel};
use crate::params::{Denoise, Geometry, PhotoStack, WhiteBalance};
use crate::temperature::WhiteScale;

/// The adjusted image is written here, and egui samples it directly.
///
/// Deliberately NOT an `Srgb` format. egui asks for a non-sRGB surface and writes
/// gamma-encoded colour into it; if this texture were sRGB the hardware would decode on
/// sample and re-encode on present, and the image would land a gamma apart from the UI
/// around it. So there is exactly one transfer function in the whole pipeline and it is
/// written by hand at the end of adjust.wgsl.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The target of a 16-bit export: half floats, which every WebGPU device can render to
/// (16-bit unorm is an optional feature, missing on most phones). In 0..=1 that is at
/// least 2048 levels per channel, eight times what 8-bit holds, before it is widened to
/// 16-bit integers for the file.
pub const DEEP_OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Linear camera RGB, half float, with the reconstructed-highlight neutral in alpha
/// (demosaic_rcd.wgsl). 16 bits is the point: 8 would quantise the headroom that makes
/// highlight recovery possible at all.
/// The Look module's bit (`params::flags::LOOK`), masked off while no table is loaded.
const LOOK_FLAG: u32 = 65536;
const SENSOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The intermediate green plane. Half float is ample — it only has to carry one channel
/// of a signal that is about to be added back to a colour difference.
const GREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// RCD's soft directions, vertical-or-horizontal and diagonal: ratios in 0..1.
const RCD_DIRS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// RCD's red at blue sites and blue at red ones, as the green plane is kept.
const RCD_OPPOSITE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct HistogramParams {
    /// xy = origin of the measured region, zw = its size.
    region: [u32; 4],
    stride: [u32; 4],
}

/// Sample every Nth pixel on both axes. 256 bins are saturated long before 24M samples,
/// and every sample is an atomic increment into one of 256 addresses.
///
/// This is the stride at full resolution; a reduced preview has fewer pixels to begin
/// with, so it is scaled down with the preview (see `render`).
const HISTOGRAM_STRIDE: u32 = 4;

/// The coarsest preview: 1/32 of full resolution on each axis. Beyond that a preview is
/// smaller than any window it could be shown in.
pub const MAX_PREVIEW_LEVEL: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DemosaicUniforms {
    /// Width, height, and the pattern's width and height.
    size: [u32; 4],
    cfa: [u32; 36],
    black: [f32; 4],
    white: [f32; 4],
}

/// Everything the adjustment chain needs, in one buffer. A slider drag rewrites exactly
/// this — a few hundred bytes — and nothing else (DESIGN.md invariant 6).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AdjustUniforms {
    cam0: [f32; 4],
    cam1: [f32; 4],
    cam2: [f32; 4],
    wb: [f32; 4],
    exposure: [f32; 4],
    filmic: [f32; 4],
    lift: [f32; 4],
    gamma: [f32; 4],
    gain: [f32; 4],
    offset: [f32; 4],
    flags: [u32; 4],
    sizes: [f32; 4],
    orient: [f32; 4],
    turns: [u32; 4],
    split: [f32; 4],
    as_shot_wb: [f32; 4],
    tonal: [f32; 4],
    vibrance: [f32; 4],
    vignette: [f32; 4],
    sharpen: [f32; 4],
    dest: [f32; 4],
    detail: [f32; 4],
    eq_red: [f32; 4],
    eq_orange: [f32; 4],
    eq_yellow: [f32; 4],
    eq_green: [f32; 4],
    eq_aqua: [f32; 4],
    eq_blue: [f32; 4],
    eq_purple: [f32; 4],
    eq_magenta: [f32; 4],
    /// The crop, normalised to the oriented frame: x, y, width, height. Framing effects
    /// (the vignette) centre on what is exported, not on the uncropped sensor.
    frame: [f32; 4],
    /// x = radial distortion k, y = fill zoom, z/w = red/blue CA scale.
    lens: [f32; 4],
    /// x = vignetting correction, stops at the corner.
    lens_b: [f32; 4],
    /// The photo's own lens correction (see `lens::FileWarp`): each plane's polynomial
    /// in two halves, then centre xy, radius and 1 when on, then red's and blue's
    /// lateral colour (v, c, b, 1 when on), then the fall-off (k1..k3, its radius).
    warp: [[f32; 4]; 6],
    warp_c: [f32; 4],
    tca_r: [f32; 4],
    tca_b: [f32; 4],
    warp_vig: [f32; 4],
    /// Local masks, five vec4 each: see `adjust.wgsl`'s `masks`.
    masks: [[f32; 4]; 40],
    /// Masks in use, 1 + the one shown as an overlay (0 for none), the dust view, spots.
    mask_info: [u32; 4],
    /// Heal spots, two vec4 each: see `adjust.wgsl`'s `spots`.
    spots: [[f32; 4]; 64],
    /// The keystone homography's rows (xyz), the first's w its unit in pixels; the second's
    /// w is 1 while there is one.
    persp: [[f32; 4]; 3],
    /// The working space's luminance weights (xyz); w = 1 for the wide working space.
    luma_w: [f32; 4],
    /// Working → output linear RGB, rows (xyz); [0].w = the output's gamma (0 for the
    /// sRGB curve), [1].w = 1 when the output's primaries are not the working space's,
    /// [2].w = 1 to finish through the screen table.
    out_m: [[f32; 4]; 3],
    /// Proofing: the proof's space → the screen's, rows; [0].w = 1 while proofing against
    /// a space, 2 while proofing through the printer table, [1].w = 1 to mark what the
    /// proof cannot hold, [2].w = the proof space's gamma (0: the sRGB curve), which the
    /// printer table's input is encoded with.
    proof_m: [[f32; 4]; 3],
    /// The gain map render (export only): the output's luminance weights (xyz) and the
    /// HDR rendition's white (w; 0 for an ordinary render).
    hdr: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ResizeUniforms {
    crop: [f32; 4],
    dest_size: [f32; 4],
}

/// A soft proof: the space proofed against, and whether to mark what it cannot hold.
/// With `printer`, the file written in `space` is then shown as printed, through the table
/// given to [`Engine::set_proof_table`] (proofing falls back to `space` alone until one is).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proof {
    pub space: crate::gamut::Screen,
    pub warn: bool,
    pub printer: bool,
}

/// Where a render ends: the space and curve it is encoded for, and a proof on the way.
#[derive(Debug, Clone, Copy)]
struct Target {
    out: crate::gamut::Screen,
    proof: Option<Proof>,
    /// Encode for `out` (Rec.2020 and the sRGB curve) and then through the screen table.
    screen_table: bool,
    /// Render the gain map's two luminances instead, the HDR rendition's white this many
    /// times the export's (see `adjust.wgsl`'s `finish`).
    hdr_white: Option<f32>,
}

pub struct LoadedImage {
    /// Full-resolution output dimensions, after right-angle turns. What export writes,
    /// and what crop and aspect are measured in. The preview texture may be smaller.
    pub width: u32,
    pub height: u32,
    /// The sensor's own dimensions, which never change once decoded.
    pub sensor_width: u32,
    pub sensor_height: u32,
    geometry: Geometry,
    pub make: String,
    pub model: String,
    /// As-shot neutral multipliers, before any user tint.
    wb: [f32; 3],
    cam_to_srgb: [[f32; 3]; 3],
    /// Camera → linear sRGB for the wide working space: the DNG's own profile where it
    /// has one (W-09), else `cam_to_srgb`.
    cam_profiled: [[f32; 3]; 3],
    /// The same as `params::Rendering::V2` draws it.
    cam_profiled_v2: [[f32; 3]; 3],
    white_scale: WhiteScale,
    /// The file's own distortion correction, if it carries one.
    warp: Option<FileWarp>,
    /// The lens's profile, if one was found; applied while Lens is on and asks for it.
    lens_profile: Option<(String, FileWarp)>,
    /// Levels in the sensor texture's mip chain; level 0 is full resolution.
    mip_levels: u32,
    /// Kept so its memory can be recycled when this image is replaced by one the same size.
    sensor: wgpu::Texture,
    /// Every level, as the adjust pass samples it; export binds it with its own uniforms.
    sensor_view: wgpu::TextureView,
    sensor_extent: wgpu::Extent3d,
    /// The preview renders at 1/2^level of full resolution. 0 = full.
    preview_level: u32,
    /// The preview: what egui draws. Sized by `preview_level`, not by the sensor.
    pub output_view: wgpu::TextureView,
    output_texture: wgpu::Texture,
    adjust_bind: wgpu::BindGroup,
    /// The histogram pass's bind group, which reads the preview: made on first use and
    /// again only when the preview is replaced, not every frame (WP5.4).
    hist_bind: std::sync::OnceLock<wgpu::BindGroup>,
    /// The clipping overlay's texture and the bind group that reads the preview into it,
    /// while the overlay is on. Preview-sized; replaced with the preview.
    overlay: Option<(wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
    /// Brush masks' coverage, which every adjust bind group reads.
    brushes: Brushes,
    /// The denoised copy the preview reads while Denoise is on, and the adjust bind group
    /// that reads it. Made from the preview's level down, so it costs 1/4^level of a
    /// full-resolution copy.
    denoised: Option<(Denoised, wgpu::BindGroup)>,
    /// The Remove spots' fills, by atlas slot (the n-th Remove spot is in slot n); `None`
    /// for one that could not be filled, which is then healed instead.
    fills: Vec<Option<Fill>>,
}

/// A Remove spot's content-aware fill: the sensor area it was made from, which sits in its
/// atlas slot (`fill_atlas`) at `scale` sensor pixels a texel.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fill {
    /// The spot it was made for: its centre and radius, as bits.
    key: [u32; 3],
    /// The window's corner, sensor px.
    origin: [f32; 2],
    scale: f32,
}

impl Fill {
    fn key(spot: &crate::heal::Spot) -> [u32; 3] {
        [
            spot.dst[0].to_bits(),
            spot.dst[1].to_bits(),
            spot.radius.to_bits(),
        ]
    }
}

/// Brush masks' coverage, one texture layer each, rasterised from their dabs (see
/// `local::rasterise`). Kept with the bytes and the dabs each layer holds, so a stroke in
/// progress only lays its new dabs and uploads what they touched.
struct Brushes {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    layers: Vec<(Vec<crate::local::Dab>, Vec<u8>)>,
}

impl Brushes {
    fn new(device: &wgpu::Device, sensor: (u32, u32)) -> Self {
        let size = crate::local::brush_size(sensor);
        let texture = device.make_texture(&wgpu::TextureDescriptor {
            label: Some("brush masks"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: crate::local::MAX_MASKS as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let blank = vec![0u8; (size.0 * size.1) as usize];
        Self {
            texture,
            view,
            size,
            layers: (0..crate::local::MAX_MASKS)
                .map(|_| (Vec::new(), blank.clone()))
                .collect(),
        }
    }

    /// Make each layer hold its brush mask's dabs: new dabs laid on top when a stroke has
    /// only grown, the layer redrawn otherwise; only what changed is uploaded.
    fn sync(&mut self, queue: &wgpu::Queue, wanted: &[&[crate::local::Dab]]) {
        let (w, h) = self.size;
        for (l, (have, buf)) in self.layers.iter_mut().enumerate() {
            let want: &[crate::local::Dab] = wanted.get(l).copied().unwrap_or(&[]);
            if want == have.as_slice() {
                continue;
            }
            let dirty = if want.starts_with(have) {
                crate::local::rasterise(buf, self.size, &want[have.len()..])
            } else {
                buf.fill(0);
                crate::local::rasterise(buf, self.size, want);
                Some((0, 0, w, h))
            };
            *have = want.to_vec();
            if let Some((x, y, rw, rh)) = dirty {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: l as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    buf,
                    wgpu::TexelCopyBufferLayout {
                        offset: (y * w + x) as u64,
                        bytes_per_row: Some(w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: rw,
                        height: rh,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
}

/// The dabs of each brush mask that is on, in the order the shader numbers them.
fn brush_layers(stack: &PhotoStack) -> Vec<&[crate::local::Dab]> {
    if !stack.local.enabled {
        return Vec::new();
    }
    stack
        .local
        .params
        .masks
        .iter()
        .filter(|m| m.on)
        .take(crate::local::MAX_MASKS)
        .filter_map(|m| match &m.shape {
            crate::local::Shape::Brush { dabs } => Some(dabs.as_slice()),
            _ => None,
        })
        .collect()
}

/// A denoised copy of the sensor from level `base` down (see `shaders/denoise.wgsl`): its
/// level 0 is the sensor's level `base`.
struct Denoised {
    base: u32,
    params: Denoise,
    local: LocalDenoise,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// One pass per level written: its uniform buffer, bind group and target. Kept so a
    /// slider drag rewrites the buffers and draws again rather than making them anew.
    passes: Vec<DenoisePass>,
}

struct DenoisePass {
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    target: wgpu::TextureView,
}

impl LoadedImage {
    /// The lens correction this photo is drawn with under `stack`: its own, if the file
    /// carries one, else its lens's profile while Lens is on and asks for it.
    pub fn warp_for(&self, stack: &PhotoStack) -> Option<FileWarp> {
        self.warp.or_else(|| {
            let wanted = stack.lens.enabled && stack.lens.params.profile;
            self.lens_profile
                .as_ref()
                .filter(|_| wanted)
                .map(|(_, w)| *w)
        })
    }

    /// Whether the file corrects its own lens (a DNG's opcodes), making a profile moot.
    pub fn corrects_its_own_lens(&self) -> bool {
        self.warp.is_some()
    }

    /// The name of the lens profile found for this photo.
    pub fn lens_profile_name(&self) -> Option<&str> {
        self.lens_profile.as_ref().map(|(name, _)| name.as_str())
    }

    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }
    /// Kelvin and tint for this camera's white balance.
    pub fn white_scale(&self) -> WhiteScale {
        self.white_scale
    }
    pub fn output_texture(&self) -> &wgpu::Texture {
        &self.output_texture
    }
    pub fn preview_level(&self) -> u32 {
        self.preview_level
    }
    /// The preview texture's size in pixels.
    pub fn preview_size(&self) -> (u32, u32) {
        scaled(self.width, self.height, self.preview_level)
    }
}

/// `width × height` reduced by 2^level, never below one pixel.
fn scaled(width: u32, height: u32, level: u32) -> (u32, u32) {
    (
        width.div_ceil(1 << level).max(1),
        height.div_ceil(1 << level).max(1),
    )
}

/// Levels needed so the coarsest one is 1/2^MAX_PREVIEW_LEVEL, limited by the image size.
fn mip_count(width: u32, height: u32) -> u32 {
    let full_chain = 32 - width.max(height).max(1).leading_zeros();
    full_chain.min(MAX_PREVIEW_LEVEL + 1)
}

fn output_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    output_texture_as(device, width, height, OUTPUT_FORMAT)
}

fn output_texture_as(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.make_texture(&wgpu::TextureDescriptor {
        label: Some("adjusted output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The shader's `FLAG_NAN_PAINT`: outside the module bits, never set by a stack.
const NAN_PAINT: u32 = 0x8000_0000;

pub struct Engine {
    /// Bayer demosaic, RCD's passes (`shaders/demosaic_rcd.wgsl`): the highlight
    /// ratios, directions, green, the opposite colour at red and blue sites, and the rest.
    rcd_pipelines: [wgpu::RenderPipeline; 5],
    rcd_bgl: wgpu::BindGroupLayout,
    rcd_sampler: wgpu::Sampler,
    green_bgl: wgpu::BindGroupLayout,
    /// Green then red and blue for Fuji's X-Trans (`shaders/demosaic_xtrans.wgsl`).
    xtrans_green_pipeline: wgpu::RenderPipeline,
    xtrans_rb_pipeline: wgpu::RenderPipeline,
    rb_bgl: wgpu::BindGroupLayout,
    adjust_pipeline: wgpu::RenderPipeline,
    /// The same shader into [`DEEP_OUTPUT_FORMAT`], for 16-bit export.
    deep_adjust_pipeline: wgpu::RenderPipeline,
    adjust_bgl: wgpu::BindGroupLayout,
    adjust_buffer: wgpu::Buffer,
    mip_pipeline: wgpu::RenderPipeline,
    clip_pipeline: wgpu::RenderPipeline,
    clip_bgl: wgpu::BindGroupLayout,
    /// See [`Engine::set_clip_overlay`].
    clip_overlay: bool,
    /// See [`Engine::set_mask_overlay`].
    mask_overlay: Option<usize>,
    /// See [`Engine::set_dust_view`].
    dust_view: bool,
    /// The screen the preview is shown on (W-14), and what it is proofed against (W-10).
    screen: crate::gamut::Screen,
    proof: Option<Proof>,
    mip_bgl: wgpu::BindGroupLayout,
    denoise_pipeline: wgpu::RenderPipeline,
    denoise_bgl: wgpu::BindGroupLayout,
    lut_texture: wgpu::Texture,
    lut_view: wgpu::TextureView,
    /// The printer proof (W-10), baked by `proof::ProofTable`, and whether one is loaded.
    proof_table: wgpu::Texture,
    proof_table_view: wgpu::TextureView,
    proof_table_loaded: bool,
    /// The Look module's `.cube`, baked (`lut3d::Lut3D::bake`), the file it was baked from,
    /// and whether it read.
    look_table: wgpu::Texture,
    look_table_view: wgpu::TextureView,
    look_path: Option<String>,
    look_loaded: bool,
    /// Remove spots' fills, a slot each (`heal::FILL_SLOTS` of `heal::FILL_TILE` texels).
    fill_atlas: wgpu::Texture,
    fill_atlas_view: wgpu::TextureView,
    /// The screen's own profile (W-14), baked the same way, and whether one is loaded.
    screen_table: wgpu::Texture,
    screen_table_view: wgpu::TextureView,
    screen_table_loaded: bool,
    sensor_sampler: wgpu::Sampler,
    // The curves the LUT was last baked from, `None` where that curve was disabled. Keyed on
    // what is actually baked, so toggling a curve off rebuilds the LUT rather than leaving
    // its old rows in place.
    cached_curve: Option<Vec<CurveNode>>,
    cached_rgb_curves: Option<crate::curve::RgbCurves>,
    image: Option<LoadedImage>,

    resize_pipeline: wgpu::RenderPipeline,
    deep_resize_pipeline: wgpu::RenderPipeline,
    resize_bgl: wgpu::BindGroupLayout,
    resize_sampler: wgpu::Sampler,
    resize_buffer: wgpu::Buffer,

    hist_pipeline: wgpu::ComputePipeline,
    hist_bgl: wgpu::BindGroupLayout,
    hist_bins: wgpu::Buffer,
    hist_staging: wgpu::Buffer,
    hist_params: wgpu::Buffer,
    hist_state: ReadbackState,
    histogram: Histogram,
    /// Bumped whenever the image changes. A readback issued before the change completes
    /// after it, and without this its stale bins would be accepted as the new image's —
    /// opening a photo would show the previous one's scopes.
    hist_epoch: u64,
    hist_inflight_epoch: u64,
    /// A render that could not be measured because a readback was still in flight, and
    /// what to measure once it is not: the region of the preview and its level. Without
    /// this the last frame of a drag was often never measured, and the scopes showed a
    /// state from partway through it until the next edit.
    hist_owed: Option<([u32; 4], u32)>,

    /// The preview level, kept across images: the window has not changed size just
    /// because the photo did, so the next one opens straight at the right resolution.
    preview_level: u32,
    /// GPU memory kept from the previous image for the next one of the same size — the
    /// normal case when stepping through a roll from one camera. Allocating ~300 MB of
    /// fresh textures per photo costs tens of milliseconds of driver time on the UI thread.
    scratch: Option<Scratch>,
    retired_sensor: Option<(wgpu::Extent3d, u32, wgpu::Texture)>,
    /// Where the mosaic texture goes once demosaic has read it, for the decode thread's
    /// next upload of the same size (see `UploadPool`).
    upload_pool: UploadPool,

    /// Export's own uniforms, so an export never overwrites what the preview was drawn
    /// with — the next frame shows the same image whether or not an export ran between.
    export_buffer: wgpu::Buffer,
    /// A smaller readback band than the device allows, for tests (`set_export_band_bytes`).
    export_band_bytes: Option<u64>,
    /// See [`Engine::set_nan_paint`].
    nan_paint: bool,
}

/// The green plane, which nothing reads once demosaic has run.
struct Scratch {
    extent: wgpu::Extent3d,
    green: wgpu::Texture,
}

/// A photo whose pixels are already in GPU memory, ready for `Engine::load_uploaded`.
///
/// Uploading is the one large CPU cost of opening a photo that is not the decode:
/// `write_texture` copies the whole image into wgpu's staging memory — 2 bytes a
/// photosite for a mosaic, 8 bytes a pixel for a raster, so 66 MB for a 33 MP RAW and
/// 192 MB for a 24 MP JPEG. `upload_cfa` and `upload_raster` do that on whatever thread
/// calls them, which in the app is the decode thread; the UI thread's share of an open is
/// then recording and submitting GPU work, with no copy of the image at all.
///
/// Ordering is safe by construction: a queue write lands before the next submission on
/// that queue, and this value can only reach `load_uploaded` after the write has returned.
pub struct UploadedImage {
    pixels: UploadedPixels,
    extent: wgpu::Extent3d,
    /// What the engine starts from: identity for a RAW, whose orientation is an edit in
    /// the stack, and the file's own for a raster.
    geometry: Geometry,
    /// How the camera says the frame is meant to be viewed.
    pub orientation: Geometry,
    wb: [f32; 3],
    cam_to_srgb: [[f32; 3]; 3],
    /// Camera → linear sRGB for the wide working space: the DNG's own profile where it
    /// has one (W-09), else `cam_to_srgb`.
    cam_profiled: [[f32; 3]; 3],
    cam_profiled_v2: [[f32; 3]; 3],
    white_scale: WhiteScale,
    warp: Option<FileWarp>,
    /// The lens's profile, by name, if the loader found one (see `lensfun::for_photo`).
    pub lens_profile: Option<(String, FileWarp)>,
    pub make: String,
    pub model: String,
}

enum UploadedPixels {
    /// The sensor mosaic, still to be demosaiced.
    Mosaic {
        texture: wgpu::Texture,
        cfa: CfaPattern,
        black: [f32; 4],
        white: [f32; 4],
    },
    /// Linear RGB half-floats, for mip level 0 of the sensor texture as they are.
    Linear { texture: wgpu::Texture },
}

impl UploadedImage {
    pub fn width(&self) -> u32 {
        self.extent.width
    }
    pub fn height(&self) -> u32 {
        self.extent.height
    }
}

/// One spare mosaic texture, shared between the engine and whichever thread uploads.
///
/// The engine returns a mosaic here once its demosaic is submitted, and the next upload
/// of the same size writes into it instead of allocating 66–130 MB afresh.
#[derive(Clone, Default)]
pub struct UploadPool(Arc<Mutex<Option<wgpu::Texture>>>);

impl UploadPool {
    fn take(&self, extent: wgpu::Extent3d) -> Option<wgpu::Texture> {
        let mut slot = self.0.lock().ok()?;
        slot.take_if(|t| t.size() == extent)
    }

    fn put(&self, texture: wgpu::Texture) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(texture);
        }
    }

    fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

fn check_size(device: &wgpu::Device, width: u32, height: u32) -> Result<()> {
    let limit = device.limits().max_texture_dimension_2d;
    if width > limit || height > limit {
        bail!("{width}×{height} exceeds this GPU's {limit}px texture limit");
    }
    Ok(())
}

/// Copy a decoded mosaic into GPU memory. Safe to call from any thread.
pub fn upload_cfa(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pool: &UploadPool,
    cfa: CfaImage,
) -> Result<UploadedImage> {
    check_size(device, cfa.width, cfa.height)?;
    let extent = wgpu::Extent3d {
        width: cfa.width,
        height: cfa.height,
        depth_or_array_layers: 1,
    };
    if let Some(rgb) = &cfa.linear {
        return upload_linear(device, queue, &cfa, rgb, extent);
    }
    let texture = scoped(device, "uploading the mosaic", || {
        let texture = pool.take(extent).unwrap_or_else(|| {
            device.make_texture(&wgpu::TextureDescriptor {
                label: Some("cfa mosaic"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R16Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&cfa.data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cfa.width * 2),
                rows_per_image: Some(cfa.height),
            },
            extent,
        );
        texture
    })?;
    Ok(UploadedImage {
        pixels: UploadedPixels::Mosaic {
            texture,
            cfa: cfa.cfa,
            black: cfa.black,
            white: cfa.white,
        },
        extent,
        geometry: Geometry::default(),
        orientation: cfa.orientation,
        wb: cfa.wb,
        cam_to_srgb: cfa.cam_to_srgb,
        cam_profiled: cfa.cam_profiled.unwrap_or(cfa.cam_to_srgb),
        cam_profiled_v2: cfa
            .cam_profiled_v2
            .or(cfa.cam_profiled)
            .unwrap_or(cfa.cam_to_srgb),
        white_scale: WhiteScale::new(cfa.wb, cfa.daylight_wb, cfa.cam_to_srgb),
        warp: cfa.warp,
        lens_profile: None,
        make: cfa.make,
        model: cfa.model,
    })
}

/// A linear DNG's RGB as the sensor texture's level 0 (no demosaic), with a RAW's colour:
/// its white balance, matrices and orientation-as-an-edit, as a mosaic would have.
fn upload_linear(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cfa: &CfaImage,
    rgb: &[f32],
    extent: wgpu::Extent3d,
) -> Result<UploadedImage> {
    if rgb.len() < extent.width as usize * extent.height as usize * 3 {
        bail!("the linear image is short of its size");
    }
    // A = 0, the reconstruction neutral a raster carries too.
    let texels: Vec<half::f16> = rgb
        .chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 0.0].map(half::f16::from_f32))
        .collect();
    let texture = scoped(device, "uploading the image", || {
        let texture = device.make_texture(&wgpu::TextureDescriptor {
            label: Some("linear dng upload"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SENSOR_FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(extent.width * 8),
                rows_per_image: Some(extent.height),
            },
            extent,
        );
        texture
    })?;
    Ok(UploadedImage {
        pixels: UploadedPixels::Linear { texture },
        extent,
        geometry: Geometry::default(),
        orientation: cfa.orientation,
        wb: cfa.wb,
        cam_to_srgb: cfa.cam_to_srgb,
        cam_profiled: cfa.cam_profiled.unwrap_or(cfa.cam_to_srgb),
        cam_profiled_v2: cfa
            .cam_profiled_v2
            .or(cfa.cam_profiled)
            .unwrap_or(cfa.cam_to_srgb),
        white_scale: WhiteScale::new(cfa.wb, cfa.daylight_wb, cfa.cam_to_srgb),
        warp: cfa.warp,
        lens_profile: None,
        make: cfa.make.clone(),
        model: cfa.model.clone(),
    })
}

/// Copy a decoded raster (JPEG, PNG, TIFF, WebP) into GPU memory. Safe to call from any
/// thread.
pub fn upload_raster(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    raster: RasterImage,
) -> Result<UploadedImage> {
    check_size(device, raster.width, raster.height)?;
    let extent = wgpu::Extent3d {
        width: raster.width,
        height: raster.height,
        depth_or_array_layers: 1,
    };
    let texture = scoped(device, "uploading the image", || {
        let texture = device.make_texture(&wgpu::TextureDescriptor {
            label: Some("raster upload"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SENSOR_FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &raster.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(raster.width * 8),
                rows_per_image: Some(raster.height),
            },
            extent,
        );
        texture
    })?;
    Ok(UploadedImage {
        pixels: UploadedPixels::Linear { texture },
        extent,
        geometry: raster.orientation,
        orientation: raster.orientation,
        wb: [1.0, 1.0, 1.0],
        cam_to_srgb: raster.cam_to_srgb,
        cam_profiled: raster.cam_to_srgb,
        cam_profiled_v2: raster.cam_to_srgb,
        white_scale: WhiteScale::srgb(),
        warp: None,
        lens_profile: None,
        make: raster.make,
        model: raster.model,
    })
}

impl Engine {
    pub fn new(device: &wgpu::Device) -> Self {
        let rcd_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("demosaic rcd"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/demosaic_rcd.wgsl").into()),
        });
        let adjust_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("adjust"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/adjust.wgsl").into()),
        });
        let mip_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sensor downsample"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/downsample.wgsl").into()),
        });
        let mip_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sensor downsample bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let clip_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clip overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/clip_mask.wgsl").into()),
        });
        let clip_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clip overlay bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let clip_pipeline = fullscreen_pipeline(
            device,
            "clip overlay",
            &clip_shader,
            &clip_bgl,
            OUTPUT_FORMAT,
        );
        let mip_pipeline = fullscreen_pipeline(
            device,
            "sensor downsample",
            &mip_shader,
            &mip_bgl,
            SENSOR_FORMAT,
        );
        let resize_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu resize"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/resize.wgsl").into()),
        });
        let resize_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu resize bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let resize_pipeline = fullscreen_pipeline(
            device,
            "export resize",
            &resize_shader,
            &resize_bgl,
            OUTPUT_FORMAT,
        );
        let deep_resize_pipeline = fullscreen_pipeline(
            device,
            "export deep resize",
            &resize_shader,
            &resize_bgl,
            DEEP_OUTPUT_FORMAT,
        );
        let resize_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("resize sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let resize_buffer = device.make_buffer(&wgpu::BufferDescriptor {
            label: Some("resize uniforms"),
            size: std::mem::size_of::<ResizeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mosaic_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };

        let green_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("demosaic green bgl"),
            entries: &[mosaic_entry, uniform_entry(1)],
        });

        let rb_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("demosaic rb bgl"),
            entries: &[
                mosaic_entry,
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let table_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D3,
                multisampled: false,
            },
            count: None,
        };
        let adjust_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("adjust bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // Filterable now: straightening lands between texels, so the
                        // sensor has to be sampled rather than loaded. Rgba16Float is
                        // filterable in core WebGPU, so this still asks for no optional
                        // feature.
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // The curve LUT is still textureLoad-only and stays unfilterable.
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The sensor as decoded, for the before half of a split.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Brush masks, a layer each.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // The printer proof table, and the screen's.
                table_entry(6),
                table_entry(7),
                // The Look module's table.
                table_entry(9),
                // Remove spots' fills.
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let denoise_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("denoise"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/denoise.wgsl").into()),
        });
        let denoise_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("denoise bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let denoise_pipeline = fullscreen_pipeline(
            device,
            "denoise",
            &denoise_shader,
            &denoise_bgl,
            SENSOR_FORMAT,
        );

        let sensor_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sensor sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            // The preview picks a whole level; there is nothing to blend between.
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // RCD's passes read the mosaic and what the passes before them wrote.
        let plane_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let rcd_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("demosaic rcd bgl"),
            entries: &[
                mosaic_entry,
                uniform_entry(1),
                plane_entry(2),
                plane_entry(3),
                plane_entry(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let rcd_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rcd ratio sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let rcd_pipelines = [
            ("fs_ratios", SENSOR_FORMAT),
            ("fs_dirs", RCD_DIRS_FORMAT),
            ("fs_green", GREEN_FORMAT),
            ("fs_opposite", RCD_OPPOSITE_FORMAT),
            ("fs_final", SENSOR_FORMAT),
        ]
        .map(|(entry, format)| {
            fullscreen_pipeline_at(device, "demosaic rcd", &rcd_shader, entry, &rcd_bgl, format)
        });
        let xtrans_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("demosaic x-trans"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/demosaic_xtrans.wgsl").into()),
        });
        let xtrans_green_pipeline = fullscreen_pipeline_at(
            device,
            "demosaic x-trans green",
            &xtrans_shader,
            "fs_green",
            &green_bgl,
            GREEN_FORMAT,
        );
        let xtrans_rb_pipeline = fullscreen_pipeline_at(
            device,
            "demosaic x-trans rb",
            &xtrans_shader,
            "fs_rb",
            &rb_bgl,
            SENSOR_FORMAT,
        );
        let adjust_pipeline =
            fullscreen_pipeline(device, "adjust", &adjust_shader, &adjust_bgl, OUTPUT_FORMAT);
        let deep_adjust_pipeline = fullscreen_pipeline(
            device,
            "adjust 16-bit",
            &adjust_shader,
            &adjust_bgl,
            DEEP_OUTPUT_FORMAT,
        );

        let uniform_buffer = |label| {
            device.make_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<AdjustUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let adjust_buffer = uniform_buffer("adjust uniforms");
        let export_buffer = uniform_buffer("export uniforms");

        let lut_texture = device.make_texture(&wgpu::TextureDescriptor {
            label: Some("tone curve lut"),
            size: wgpu::Extent3d {
                width: LUT_SIZE as u32,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lut_view = lut_texture.create_view(&Default::default());
        let table_3d = |label| {
            let side = crate::proof::SIZE as u32;
            device.make_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: side,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                // Filterable in core WebGPU, so the shader interpolates between samples.
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let proof_table = table_3d("printer proof table");
        let proof_table_view = proof_table.create_view(&Default::default());
        let screen_table = table_3d("screen profile table");
        let screen_table_view = screen_table.create_view(&Default::default());
        let look_table = table_3d("look table");
        let look_table_view = look_table.create_view(&Default::default());
        let fill_atlas = device.make_texture(&wgpu::TextureDescriptor {
            label: Some("remove fill atlas"),
            size: wgpu::Extent3d {
                width: crate::heal::FILL_ATLAS,
                height: crate::heal::FILL_ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SENSOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let fill_atlas_view = fill_atlas.create_view(&Default::default());

        let hist_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("histogram"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/histogram.wgsl").into()),
        });
        let hist_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("histogram bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let hist_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("histogram"),
            bind_group_layouts: &[Some(&hist_bgl)],
            immediate_size: 0,
        });
        let hist_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("histogram"),
            layout: Some(&hist_layout),
            module: &hist_shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let hist_bins = device.make_buffer(&wgpu::BufferDescriptor {
            label: Some("histogram bins"),
            size: BUFFER_BYTES,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hist_staging = device.make_buffer(&wgpu::BufferDescriptor {
            label: Some("histogram staging"),
            size: BUFFER_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let hist_params = device.make_buffer(&wgpu::BufferDescriptor {
            label: Some("histogram params"),
            size: std::mem::size_of::<HistogramParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            hist_pipeline,
            hist_bgl,
            hist_bins,
            hist_staging,
            hist_params,
            hist_state: ReadbackState::new(),
            histogram: Histogram::default(),
            hist_epoch: 0,
            hist_inflight_epoch: 0,
            hist_owed: None,
            rcd_pipelines,
            rcd_bgl,
            rcd_sampler,
            green_bgl,
            xtrans_green_pipeline,
            xtrans_rb_pipeline,
            rb_bgl,
            adjust_pipeline,
            deep_adjust_pipeline,
            adjust_bgl,
            adjust_buffer,
            mip_pipeline,
            mip_bgl,
            denoise_pipeline,
            denoise_bgl,
            lut_texture,
            lut_view,
            proof_table,
            proof_table_view,
            proof_table_loaded: false,
            screen_table,
            screen_table_view,
            screen_table_loaded: false,
            fill_atlas,
            fill_atlas_view,
            look_table,
            look_table_view,
            look_path: None,
            look_loaded: false,
            sensor_sampler,
            cached_curve: None,
            cached_rgb_curves: None,
            image: None,
            resize_pipeline,
            deep_resize_pipeline,
            resize_bgl,
            resize_sampler,
            resize_buffer,
            preview_level: 0,
            scratch: None,
            retired_sensor: None,
            upload_pool: UploadPool::default(),
            export_buffer,
            export_band_bytes: None,
            nan_paint: false,
            clip_pipeline,
            clip_bgl,
            clip_overlay: false,
            mask_overlay: None,
            dust_view: false,
            screen: crate::gamut::Screen::SRGB,
            proof: None,
        }
    }

    /// Show the preview as its finest detail alone, so sensor dust stands out. Never in an
    /// export.
    pub fn set_dust_view(&mut self, on: bool) {
        self.dust_view = on;
    }

    /// The screen the preview is for (W-14): its primaries and curve. Exports are not
    /// touched; they are written in their own space.
    pub fn set_screen(&mut self, screen: crate::gamut::Screen) {
        self.screen = screen;
    }

    /// Proof the preview against an output space (W-10): colours it cannot hold are
    /// brought in as the export will bring them, and with `warn`, shown grey.
    pub fn set_proof(&mut self, proof: Option<Proof>) {
        self.proof = proof;
    }

    /// The printer proof a [`Proof`] with `printer` shows through, baked for this screen
    /// (`None` unloads it). Written into a table made once, so changing it makes nothing.
    pub fn set_proof_table(
        &mut self,
        queue: &wgpu::Queue,
        table: Option<&crate::proof::ProofTable>,
    ) {
        self.proof_table_loaded = table.is_some();
        if let Some(table) = table {
            write_table(queue, &self.proof_table, table);
        }
    }

    /// The screen's own profile, baked by `ProofTable::bake_screen`, for the preview to
    /// finish through instead of the screen's matrix and curve (`None` goes back to them).
    /// Exports never use it.
    pub fn set_screen_table(
        &mut self,
        queue: &wgpu::Queue,
        table: Option<&crate::proof::ProofTable>,
    ) {
        self.screen_table_loaded = table.is_some();
        if let Some(table) = table {
            write_table(queue, &self.screen_table, table);
        }
    }

    /// Wash local mask `index` (of the stack's masks) red on the preview, to show where it
    /// applies. Never in an export.
    pub fn set_mask_overlay(&mut self, index: Option<usize>) {
        self.mask_overlay = index;
    }

    /// Show clipping on the preview: every pixel at white red, at black blue. Drawn into a
    /// texture of its own, so the preview the histogram measures and export equals is
    /// untouched. Returns true when [`Self::display_view`] changed, which, like a new
    /// preview texture, means re-registering it with the UI.
    pub fn set_clip_overlay(&mut self, device: &wgpu::Device, on: bool) -> bool {
        if self.clip_overlay == on {
            return false;
        }
        self.clip_overlay = on;
        self.sync_overlay(device);
        self.image.is_some()
    }

    pub fn clip_overlay(&self) -> bool {
        self.clip_overlay
    }

    /// What the stage should show: the preview, or the preview with its clipping painted.
    pub fn display_view(&self) -> Option<&wgpu::TextureView> {
        let image = self.image.as_ref()?;
        Some(match &image.overlay {
            Some((_, view, _)) => view,
            None => &image.output_view,
        })
    }

    /// The texture behind [`Self::display_view`].
    pub fn display_texture(&self) -> Option<&wgpu::Texture> {
        let image = self.image.as_ref()?;
        Some(match &image.overlay {
            Some((texture, _, _)) => texture,
            None => &image.output_texture,
        })
    }

    /// Give the loaded image an overlay texture matching its preview while the overlay is
    /// on, and none while it is off.
    fn sync_overlay(&mut self, device: &wgpu::Device) {
        let on = self.clip_overlay;
        let Some(image) = &mut self.image else {
            return;
        };
        if !on {
            image.overlay = None;
            return;
        }
        let (w, h) = image.preview_size();
        let texture = output_texture(device, w, h);
        let view = texture.create_view(&Default::default());
        let bind = device.make_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clip overlay bind"),
            layout: &self.clip_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&image.output_view),
            }],
        });
        image.overlay = Some((texture, view, bind));
    }

    /// Paint any pixel whose value went NaN or infinite anywhere in the chain transparent
    /// magenta (alpha 0, which no real pixel has), instead of letting the final clamp turn
    /// it into a black speck. For tests.
    #[doc(hidden)]
    pub fn set_nan_paint(&mut self, on: bool) {
        self.nan_paint = on;
    }

    /// Read exports back in bands of at most `bytes` rather than the device's own limit.
    /// For tests, so a small image still exercises a many-band readback; `None` restores
    /// the default of `min(max_buffer_size, 64 MiB)`.
    #[doc(hidden)]
    pub fn set_export_band_bytes(&mut self, bytes: Option<u64>) {
        self.export_band_bytes = bytes;
    }

    /// The adjust pass's inputs: what it samples as the sensor (the sensor, or its denoised
    /// copy), one of the two uniform buffers, the curve LUT, and the sensor as decoded.
    fn adjust_bind(
        &self,
        device: &wgpu::Device,
        sensor_view: &wgpu::TextureView,
        original_view: &wgpu::TextureView,
        brushes: &wgpu::TextureView,
        uniforms: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.make_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adjust bind"),
            layout: &self.adjust_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(sensor_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sensor_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(original_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(brushes),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.proof_table_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.screen_table_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&self.fill_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&self.look_table_view),
                },
            ],
        })
    }

    /// `params` applied to the sensor from level `base` down, into `reuse`'s texture when
    /// it has the same shape (a slider drag re-renders; it does not reallocate).
    fn make_denoised(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params: &Denoise,
        local: &LocalDenoise,
        base: u32,
        reuse: Option<Denoised>,
    ) -> Result<Denoised> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let base = base.min(image.mip_levels - 1);
        let levels = image.mip_levels - base;
        let u = denoise_uniforms(
            params,
            image.mip_levels,
            local,
            (image.sensor_width, image.sensor_height),
        );
        let level_uniforms = |k: u32| {
            let mut uniforms = u;
            uniforms.a[0] = (base + k) as f32;
            uniforms
        };
        let (texture, view, passes) = match reuse {
            Some(d) if d.base == base => {
                for (k, pass) in (0..levels).zip(&d.passes) {
                    queue.write_buffer(&pass.uniforms, 0, bytemuck::bytes_of(&level_uniforms(k)));
                }
                (d.texture, d.view, d.passes)
            }
            _ => {
                let texture = scoped(device, "the denoised copy", || {
                    device.make_texture(&wgpu::TextureDescriptor {
                        label: Some("denoised sensor"),
                        // The sensor's own level `base`: mip sizes round down.
                        size: wgpu::Extent3d {
                            width: (image.sensor_width >> base).max(1),
                            height: (image.sensor_height >> base).max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: levels,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: SENSOR_FORMAT,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                })?;
                let view = texture.create_view(&Default::default());
                let passes = (0..levels)
                    .map(|k| self.denoise_pass(device, image, &texture, k, &level_uniforms(k)))
                    .collect();
                (texture, view, passes)
            }
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("denoise encoder"),
        });
        for pass in &passes {
            draw_fullscreen(
                &mut encoder,
                "denoise pass",
                &self.denoise_pipeline,
                &pass.bind,
                &pass.target,
            );
        }
        queue.submit([encoder.finish()]);
        Ok(Denoised {
            base,
            params: *params,
            local: local.clone(),
            texture,
            view,
            passes,
        })
    }

    /// The denoise pass that writes level `k` of `texture`, with its uniforms set.
    fn denoise_pass(
        &self,
        device: &wgpu::Device,
        image: &LoadedImage,
        texture: &wgpu::Texture,
        k: u32,
        uniforms: &DenoiseUniforms,
    ) -> DenoisePass {
        let buffer = device.make_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("denoise params"),
            contents: bytemuck::bytes_of(uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind = device.make_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("denoise bind"),
            layout: &self.denoise_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&image.sensor_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sensor_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&image.brushes.view),
                },
            ],
        });
        let target = texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: k,
            mip_level_count: Some(1),
            ..Default::default()
        });
        DenoisePass {
            uniforms: buffer,
            bind,
            target,
        }
    }

    /// Bring the preview's denoised copy in line with `stack` and the preview level: made
    /// when Denoise turns on or changes, dropped when it turns off.
    /// Read and bake the Look module's `.cube` when the file it names changes. Read here,
    /// not by the app, so every engine (the batch exporter's too) applies the same look.
    /// A file that does not read is logged and leaves the look off.
    fn sync_look(&mut self, queue: &wgpu::Queue, stack: &PhotoStack) {
        let wanted = stack
            .look
            .enabled
            .then(|| stack.look.params.path.clone())
            .flatten();
        if wanted == self.look_path {
            return;
        }
        self.look_loaded = false;
        if let Some(path) = &wanted {
            match std::fs::read_to_string(path)
                .map_err(anyhow::Error::from)
                .and_then(|text| crate::lut3d::parse_cube(&text))
            {
                Ok(lut) => {
                    write_table(queue, &self.look_table, &lut.bake());
                    self.look_loaded = true;
                }
                Err(e) => log::warn!("the look {path} could not be used: {e:#}"),
            }
        }
        self.look_path = wanted;
    }

    /// Whether the Look module's file read, so the look is being applied.
    pub fn look_loaded(&self) -> bool {
        self.look_loaded
    }

    /// Fill the Remove spots that are new or have moved, each into its atlas slot. A fill
    /// is kept as long as its spot stays put, so only placing or moving one costs a fill.
    fn sync_fills(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stack: &PhotoStack) {
        let removes: Vec<crate::heal::Spot> = if stack.heal.enabled {
            stack
                .heal
                .params
                .spots
                .iter()
                .take(crate::heal::MAX_SPOTS)
                .filter(|s| s.mode == crate::heal::Mode::Remove)
                .take(crate::heal::FILL_SLOTS)
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        let Some(image) = &self.image else { return };
        let stale: Vec<usize> = (0..removes.len())
            .filter(|&slot| {
                image
                    .fills
                    .get(slot)
                    .and_then(|f| f.as_ref())
                    .map(|f| f.key)
                    != Some(Fill::key(&removes[slot]))
            })
            .collect();
        let mut made = Vec::new();
        for slot in stale {
            let fill = self.fill_spot(device, queue, image, &removes[slot], slot);
            if let Err(e) = &fill {
                log::warn!("a Remove spot could not be filled ({e:#}); healed instead");
            }
            made.push((slot, fill.ok()));
        }
        let Some(image) = &mut self.image else { return };
        image.fills.resize(removes.len(), None);
        for (slot, fill) in made {
            image.fills[slot] = fill;
        }
    }

    /// Fill `spot` from what is around it: the sensor read back around it, at the level
    /// where that fits one atlas slot, completed by PatchMatch (lc-inpaint), and written
    /// into slot `slot`. A small spot is filled at full resolution; a large one at a
    /// coarser level, so its fill is softer than the photo around it.
    fn fill_spot(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &LoadedImage,
        spot: &crate::heal::Spot,
        slot: usize,
    ) -> Result<Fill> {
        use crate::heal::{FILL_CONTEXT, FILL_TILE};
        let (sw, sh) = (image.sensor_width, image.sensor_height);
        let r = (spot.radius * sw.max(sh) as f32).max(1.0);
        let side = r * FILL_CONTEXT;
        let mut level = 0u32;
        while level + 1 < image.mip_levels && side / (1u32 << level) as f32 > FILL_TILE as f32 {
            level += 1;
        }
        let scale = (1u32 << level) as f32;
        let (lw, lh) = ((sw >> level).max(1), (sh >> level).max(1));
        let tile = ((side / scale).ceil() as u32)
            .clamp(1, FILL_TILE)
            .min(lw)
            .min(lh);
        let centre = [
            spot.dst[0] * sw as f32 / scale,
            spot.dst[1] * sh as f32 / scale,
        ];
        let corner = |c: f32, extent: u32| {
            (c - tile as f32 / 2.0)
                .round()
                .clamp(0.0, (extent - tile) as f32) as u32
        };
        let (ox, oy) = (corner(centre[0], lw), corner(centre[1], lh));
        let texels = read_back_level(
            device,
            queue,
            &image.sensor,
            level,
            (ox, oy, tile, tile),
            8,
            MAX_BAND_BYTES,
        )?;
        // Matched in a perceptual-ish domain: in linear light, patch distances are all
        // about the highlights.
        let img: Vec<f32> = texels
            .chunks_exact(2)
            .map(|h| {
                half::f16::from_le_bytes([h[0], h[1]])
                    .to_f32()
                    .max(0.0)
                    .sqrt()
            })
            .collect();
        let hole_r = r / scale * 1.02 + 0.5;
        let n = tile as usize;
        let hole: Vec<bool> = (0..n * n)
            .map(|i| {
                let dx = (i % n) as f32 + ox as f32 + 0.5 - centre[0];
                let dy = (i / n) as f32 + oy as f32 + 0.5 - centre[1];
                dx * dx + dy * dy <= hole_r * hole_r
            })
            .collect();
        let filled = lc_inpaint::complete(n, n, 4, &img, &hole, &Default::default())
            .unwrap_or_else(|| lc_inpaint::membrane_fill(n, n, 4, &img, &hole));
        let bytes: Vec<u8> = filled
            .iter()
            .flat_map(|v| half::f16::from_f32(v * v).to_le_bytes())
            .collect();
        let per_row = crate::heal::FILL_ATLAS / FILL_TILE;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.fill_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: (slot as u32 % per_row) * FILL_TILE,
                    y: (slot as u32 / per_row) * FILL_TILE,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(tile * 8),
                rows_per_image: Some(tile),
            },
            wgpu::Extent3d {
                width: tile,
                height: tile,
                depth_or_array_layers: 1,
            },
        );
        Ok(Fill {
            key: Fill::key(spot),
            origin: [ox as f32 * scale, oy as f32 * scale],
            scale,
        })
    }

    fn sync_denoise(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stack: &PhotoStack) {
        let Some(image) = &mut self.image else { return };
        let Some((params, local)) =
            denoise_wanted(stack, (image.sensor_width, image.sensor_height))
        else {
            image.denoised = None;
            return;
        };
        let base = image.preview_level.min(image.mip_levels - 1);
        if image
            .denoised
            .as_ref()
            .is_some_and(|(d, _)| d.base == base && d.params == params && d.local == local)
        {
            return;
        }
        // The same base keeps the texture, so the adjust bind group made for it still holds.
        let (reuse, kept_bind) = match image.denoised.take() {
            Some((d, bind)) if d.base == base => (Some(d), Some(bind)),
            Some((d, _)) => (Some(d), None),
            None => (None, None),
        };
        match self.make_denoised(device, queue, &params, &local, base, reuse) {
            Ok(d) => {
                let Some(image) = &self.image else { return };
                let bind = kept_bind.unwrap_or_else(|| {
                    self.adjust_bind(
                        device,
                        &d.view,
                        &image.sensor_view,
                        &image.brushes.view,
                        &self.adjust_buffer,
                    )
                });
                if let Some(image) = &mut self.image {
                    image.denoised = Some((d, bind));
                }
            }
            // The preview shows the photo without noise reduction rather than nothing.
            Err(e) => log::error!("{e:#}; previewing without noise reduction"),
        }
    }

    fn export_band_bytes(&self, device: &wgpu::Device) -> u64 {
        let cap = device.limits().max_buffer_size.min(MAX_BAND_BYTES);
        self.export_band_bytes.map_or(cap, |b| b.min(cap)).max(1)
    }

    pub fn image(&self) -> Option<&LoadedImage> {
        self.image.as_ref()
    }

    /// Let go of the textures kept only to be reused (the green plane, the pooled mosaic,
    /// the last sensor), when the system is short of memory. The next open allocates them
    /// again.
    pub fn release_spares(&mut self) {
        self.scratch = None;
        self.retired_sensor = None;
        self.upload_pool.clear();
    }

    pub fn unload(&mut self) {
        if let Some(image) = self.image.take() {
            // Only its memory is kept, for the next image to overwrite. The image itself
            // is gone, so nothing it showed can change (invariant 5 is per image).
            self.retired_sensor = Some((image.sensor_extent, image.mip_levels, image.sensor));
        }
        self.hist_epoch = self.hist_epoch.wrapping_add(1);
        self.histogram = Histogram::default();
    }

    /// The pool `upload_cfa` should draw its mosaic texture from, so that a texture this
    /// engine has finished with is reused rather than reallocated.
    pub fn upload_pool(&self) -> UploadPool {
        self.upload_pool.clone()
    }

    /// Upload, demosaic, and keep only the linear RGB result. `upload_cfa` followed by
    /// `load_uploaded`, for callers with nothing to gain from splitting them.
    pub fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cfa: CfaImage,
    ) -> Result<()> {
        let uploaded = upload_cfa(device, queue, &self.upload_pool, cfa)?;
        self.load_uploaded(device, queue, uploaded)
    }

    /// Upload a decoded raster image (JPEG, PNG, TIFF, WebP) as the sensor texture.
    /// `upload_raster` followed by `load_uploaded`.
    pub fn load_raster(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        raster: RasterImage,
    ) -> Result<()> {
        let uploaded = upload_raster(device, queue, raster)?;
        self.load_uploaded(device, queue, uploaded)
    }

    /// Make an uploaded photo the current image: demosaic a mosaic (a raster is copied
    /// as it is), build the preview mip chain, and keep only the linear RGB result.
    ///
    /// GPU work only — the image's bytes were copied by `upload_cfa` / `upload_raster`.
    /// The mosaic goes back to the upload pool once demosaic is submitted: it has served
    /// its purpose and the sensor texture is immutable from here (invariant 5).
    pub fn load_uploaded(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uploaded: UploadedImage,
    ) -> Result<()> {
        let UploadedImage {
            pixels,
            extent,
            geometry,
            orientation: _,
            wb,
            cam_to_srgb,
            cam_profiled,
            cam_profiled_v2,
            white_scale,
            warp,
            lens_profile,
            make,
            model,
        } = uploaded;
        let (width, height) = (extent.width, extent.height);

        // Everything this allocates — sensor, mips, green plane, preview — is the most a
        // photo ever asks of the GPU. A refusal is an `Err` for the caller to show, and the
        // image already loaded stays loaded.
        let scopes = ErrorScopes::push(device);

        let mip_levels = mip_count(width, height);
        let sensor = match self.retired_sensor.take() {
            Some((e, m, texture)) if e == extent && m == mip_levels => texture,
            _ => device.make_texture(&wgpu::TextureDescriptor {
                label: Some("sensor rgb"),
                size: extent,
                mip_level_count: mip_levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SENSOR_FORMAT,
                // COPY_SRC for the eyedropper and the Auto buttons' one-shot reads.
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
        };
        // All levels, for the adjust pass to sample from; one view per level to render into.
        let sensor_view = sensor.create_view(&Default::default());
        let level_view = |level: u32| {
            sensor.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("load encoder"),
        });

        let mut spent_mosaic = None;
        match pixels {
            UploadedPixels::Mosaic {
                texture: mosaic,
                cfa,
                black,
                white,
            } => {
                let scratch = match self.scratch.take() {
                    Some(s) if s.extent == extent => s,
                    _ => Scratch {
                        extent,
                        green: device.make_texture(&wgpu::TextureDescriptor {
                            label: Some("green plane"),
                            size: extent,
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: GREEN_FORMAT,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::RENDER_ATTACHMENT,
                            view_formats: &[],
                        }),
                    },
                };
                let green_view = scratch.green.create_view(&Default::default());

                let demosaic_uniforms =
                    device.make_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("demosaic uniforms"),
                        contents: bytemuck::bytes_of(&DemosaicUniforms {
                            size: [width, height, cfa.width, cfa.height],
                            cfa: cfa.colours.map(u32::from),
                            black,
                            white,
                        }),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

                let mosaic_view = mosaic.create_view(&Default::default());
                let green_bind = device.make_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("demosaic green bind"),
                    layout: &self.green_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&mosaic_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: demosaic_uniforms.as_entire_binding(),
                        },
                    ],
                });
                let rb_bind = device.make_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("demosaic rb bind"),
                    layout: &self.rb_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&mosaic_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: demosaic_uniforms.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&green_view),
                        },
                    ],
                });

                if cfa.is_bayer() {
                    self.demosaic_rcd(
                        device,
                        &mut encoder,
                        &mosaic_view,
                        &demosaic_uniforms,
                        &green_view,
                        &level_view(0),
                        extent,
                    );
                } else {
                    // Green first and alone: red and blue are reconstructed relative to it.
                    draw_fullscreen(
                        &mut encoder,
                        "demosaic x-trans green pass",
                        &self.xtrans_green_pipeline,
                        &green_bind,
                        &green_view,
                    );
                    draw_fullscreen(
                        &mut encoder,
                        "demosaic x-trans rb pass",
                        &self.xtrans_rb_pipeline,
                        &rb_bind,
                        &level_view(0),
                    );
                }
                // Kept for the next photo's demosaic; the mosaic for its next upload.
                self.scratch = Some(scratch);
                spent_mosaic = Some(mosaic);
            }
            UploadedPixels::Linear { texture } => {
                encoder.copy_texture_to_texture(
                    texture.as_image_copy(),
                    sensor.as_image_copy(),
                    extent,
                );
            }
        }

        // Each level from the one above. Still part of "demosaic once": after this the
        // whole sensor texture, every level of it, is immutable (invariant 5).
        for level in 1..mip_levels {
            let parent = level_view(level - 1);
            let bind = device.make_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sensor downsample bind"),
                layout: &self.mip_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&parent),
                }],
            });
            draw_fullscreen(
                &mut encoder,
                "sensor downsample pass",
                &self.mip_pipeline,
                &bind,
                &level_view(level),
            );
        }
        queue.submit([encoder.finish()]);
        // Only after the submit: an upload into it now is ordered after this demosaic.
        if let Some(mosaic) = spent_mosaic {
            self.upload_pool.put(mosaic);
        }

        let (out_w, out_h) = geometry.output_size(width, height);
        let preview_level = self.preview_level.min(mip_levels - 1);
        let (preview_w, preview_h) = scaled(out_w, out_h, preview_level);
        let output_texture = output_texture(device, preview_w, preview_h);
        let output_view = output_texture.create_view(&Default::default());

        let brushes = Brushes::new(device, (width, height));
        let adjust_bind = self.adjust_bind(
            device,
            &sensor_view,
            &sensor_view,
            &brushes.view,
            &self.adjust_buffer,
        );
        scopes.pop("loading the image onto the GPU")?;

        // Any histogram in flight describes the previous image.
        self.hist_epoch = self.hist_epoch.wrapping_add(1);
        self.histogram = Histogram::default();

        self.image = Some(LoadedImage {
            width: out_w,
            height: out_h,
            sensor_width: width,
            sensor_height: height,
            geometry,
            make,
            model,
            wb,
            cam_to_srgb,
            cam_profiled,
            cam_profiled_v2,
            white_scale,
            warp,
            lens_profile,
            mip_levels,
            preview_level,
            sensor_extent: extent,
            sensor,
            sensor_view,
            output_view,
            output_texture,
            adjust_bind,
            hist_bind: std::sync::OnceLock::new(),
            overlay: None,
            brushes,
            denoised: None,
            fills: Vec::new(),
        });
        self.sync_overlay(device);
        Ok(())
    }

    /// Render the stack at full resolution and read it back off the GPU.
    ///
    /// Same shader, same uniforms as the preview; only the render target and the sensor
    /// level differ (invariant 2, and DESIGN.md "Preview and export"). The full-resolution
    /// target exists only for the length of this call, so a preview-sized session never
    /// holds a second copy of a 24 MP frame.
    ///
    /// Everything about the frame comes from `stack`: its turns set the output size and
    /// its crop the region written, whatever `set_geometry` was last told.
    ///
    /// Blocks until the GPU has finished and every band is read — at most `GPU_TIMEOUT`
    /// per wait, so a lost device is an error rather than a hang. At 24 MP the readback is
    /// ~98 MB, so this belongs off the UI thread in anything interactive. Export has its
    /// own uniforms, so the preview is untouched and needs no re-render afterwards.
    ///
    /// A GPU that refuses an allocation or a command here returns an `Err`; it does not
    /// panic (B6), since losing the app on an export would lose every unsaved edit with it.
    pub fn export(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
    ) -> Result<ExportedImage> {
        self.export_8(device, queue, stack, crate::gamut::OutputSpace::Srgb)
    }

    fn export_8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        space: crate::gamut::OutputSpace,
    ) -> Result<ExportedImage> {
        let (width, height, rgba) = self.render_export(device, queue, stack, false, space)?;
        Ok(ExportedImage {
            width,
            height,
            rgba,
        })
    }

    /// [`Self::export`] at 16 bits per channel, undithered: for PNG and TIFF masters that
    /// will be edited further, where 8 bits would band under a later curve.
    pub fn export_16(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
    ) -> Result<ExportedImage<u16>> {
        self.export_16_in(device, queue, stack, crate::gamut::OutputSpace::Srgb)
    }

    fn export_16_in(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        space: crate::gamut::OutputSpace,
    ) -> Result<ExportedImage<u16>> {
        let (width, height, halves) = self.render_export(device, queue, stack, true, space)?;
        let rgba = halves
            .chunks_exact(2)
            .map(|h| {
                let v = half::f16::from_le_bytes([h[0], h[1]]).to_f32();
                (v.clamp(0.0, 1.0) * 65535.0).round() as u16
            })
            .collect();
        Ok(ExportedImage {
            width,
            height,
            rgba,
        })
    }

    /// [`Self::export`] at the depth `deep` asks for.
    pub fn export_at(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        depth: crate::export::BitDepth,
    ) -> Result<crate::export::Exported> {
        self.export_in(device, queue, stack, depth, crate::gamut::OutputSpace::Srgb)
    }

    /// [`Self::export_at`], written in `space` (W-09): what the file's ICC profile says.
    pub fn export_in(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        depth: crate::export::BitDepth,
        space: crate::gamut::OutputSpace,
    ) -> Result<crate::export::Exported> {
        self.export_sized_in(device, queue, stack, depth, space, None)
    }

    /// Export `stack` with the requested settings, accelerating resizing on the GPU where possible.
    pub fn export_with_settings(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        settings: &crate::export::ExportSettings,
    ) -> Result<crate::export::Exported> {
        let (crop_w, crop_h) = if let Some(image) = &self.image {
            let (out_w, out_h) = stack
                .geometry
                .output_size(image.sensor_width, image.sensor_height);
            let (_, _, w, h) = stack.geometry.crop.to_pixels(out_w, out_h);
            (w, h)
        } else {
            (1, 1)
        };
        let target = settings.sizing.target(crop_w, crop_h);
        let target_size = if target != (crop_w, crop_h) {
            Some(target)
        } else {
            None
        };
        self.export_sized_in(
            device,
            queue,
            stack,
            settings.effective_depth(),
            settings.color_space,
            target_size,
        )
    }

    /// Export `stack` at `depth` in `space`, with optional GPU resizing to `target_size`.
    pub fn export_sized_in(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        depth: crate::export::BitDepth,
        space: crate::gamut::OutputSpace,
        target_size: Option<(u32, u32)>,
    ) -> Result<crate::export::Exported> {
        use crate::export::{BitDepth, Exported};
        Ok(match depth {
            BitDepth::Eight => {
                let (width, height, rgba) =
                    self.render_export_sized(device, queue, stack, false, space, target_size)?;
                Exported::Eight(ExportedImage {
                    width,
                    height,
                    rgba,
                })
            }
            BitDepth::Sixteen => {
                let (width, height, halves) =
                    self.render_export_sized(device, queue, stack, true, space, target_size)?;
                let rgba = halves
                    .chunks_exact(2)
                    .map(|h| {
                        let v = half::f16::from_le_bytes([h[0], h[1]]).to_f32();
                        (v.clamp(0.0, 1.0) * 65535.0).round() as u16
                    })
                    .collect();
                Exported::Sixteen(ExportedImage {
                    width,
                    height,
                    rgba,
                })
            }
        })
    }

    /// The HDR gain map of `stack`'s export in `space` (ISO 21496-1): rendered once more,
    /// in bands, as the luminance of the export and of its HDR rendition, whose white is
    /// [`crate::gainmap::HDR_WHITE`] times brighter, and reduced to a map a quarter the
    /// size each way as the bands come back, so the HDR rendition is never held whole.
    /// Measured on the export before it is resized: the map is stretched over the image,
    /// whatever its size.
    pub fn gain_map(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        space: crate::gamut::OutputSpace,
    ) -> Result<crate::gainmap::GainMap> {
        if let Some(image) = &mut self.image {
            image.brushes.sync(queue, &brush_layers(stack));
        }
        self.sync_fills(device, queue, stack);
        self.sync_look(queue, stack);
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let (out_w, out_h) = stack
            .geometry
            .output_size(image.sensor_width, image.sensor_height);
        check_size(device, out_w, out_h)?;
        let denoised = match denoise_wanted(stack, (image.sensor_width, image.sensor_height)) {
            Some((params, local)) => {
                Some(self.make_denoised(device, queue, &params, &local, 0, None)?)
            }
            None => None,
        };
        let sizes = ((out_w, out_h), (out_w, out_h), 0, 0);
        let target = Target {
            out: crate::gamut::Screen::of(space),
            proof: None,
            screen_table: false,
            hdr_white: Some(crate::gainmap::HDR_WHITE),
        };
        self.write_uniforms(queue, stack, None, sizes, true, false, target);
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let rect = stack.geometry.crop.to_pixels(out_w, out_h);
        let mut map = crate::gainmap::Measure::new(rect.2, rect.3);
        let sensor = denoised.as_ref().map_or(&image.sensor_view, |d| &d.view);
        let texel = DEEP_OUTPUT_FORMAT.block_copy_size(None).unwrap_or(8);
        self.render_bands_into(
            device,
            queue,
            image,
            sensor,
            &self.deep_adjust_pipeline,
            DEEP_OUTPUT_FORMAT,
            rect,
            texel,
            &mut |first, band| {
                // RGBA16F: the export's luminance in R, the HDR rendition's in G.
                let px = band.chunks_exact(8).map(|t| {
                    let h = |i: usize| half::f16::from_le_bytes([t[i], t[i + 1]]).to_f32();
                    (h(0), h(2))
                });
                map.add(first, px);
            },
        )?;
        Ok(map.finish())
    }

    fn render_export(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        deep: bool,
        space: crate::gamut::OutputSpace,
    ) -> Result<(u32, u32, Vec<u8>)> {
        self.render_export_sized(device, queue, stack, deep, space, None)
    }

    /// Render `stack` at full resolution (with optional GPU downsampling to `target_size`)
    /// and read back the crop, as tightly packed rows of the target's own texels: RGBA8,
    /// or RGBA16F little-endian when `deep`.
    fn render_export_sized(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        deep: bool,
        space: crate::gamut::OutputSpace,
        target_size: Option<(u32, u32)>,
    ) -> Result<(u32, u32, Vec<u8>)> {
        if let Some(image) = &mut self.image {
            image.brushes.sync(queue, &brush_layers(stack));
        }
        self.sync_fills(device, queue, stack);
        self.sync_look(queue, stack);
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        // Sized from the stack's own geometry, not the image's: only `set_geometry` moves
        // those, and batch export never calls it, so a portrait RAW came out rendered into a
        // landscape frame — cut top and bottom, black at the sides, the crop misplaced.
        let (out_w, out_h) = stack
            .geometry
            .output_size(image.sensor_width, image.sensor_height);
        check_size(device, out_w, out_h)?;
        // Noise reduction at full resolution, for this export only.
        let denoised = match denoise_wanted(stack, (image.sensor_width, image.sensor_height)) {
            Some((params, local)) => {
                Some(self.make_denoised(device, queue, &params, &local, 0, None)?)
            }
            None => None,
        };
        let sizes = ((out_w, out_h), (out_w, out_h), 0, 0);
        // 8-bit is dithered like the preview; 16-bit has no steps to hide.
        let target = Target {
            out: crate::gamut::Screen::of(space),
            proof: None,
            screen_table: false,
            hdr_white: None,
        };
        self.write_uniforms(queue, stack, None, sizes, true, !deep, target);

        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let (format, pipeline) = if deep {
            (DEEP_OUTPUT_FORMAT, &self.deep_adjust_pipeline)
        } else {
            (OUTPUT_FORMAT, &self.adjust_pipeline)
        };
        // The crop is not baked into the texture, so export is where it becomes real: only
        // the region the user framed is drawn. See `params::Crop` for why it is not in the
        // shader.
        let rect = stack.geometry.crop.to_pixels(out_w, out_h);
        let texel = format.block_copy_size(None).unwrap_or(4);
        let resizing =
            target_size.is_some_and(|(tw, th)| (tw < rect.2 || th < rect.3) && tw > 0 && th > 0);
        if !resizing {
            let sensor = denoised.as_ref().map_or(&image.sensor_view, |d| &d.view);
            let pixels =
                self.render_bands(device, queue, image, sensor, pipeline, format, rect, texel)?;
            return Ok((rect.2, rect.3, pixels));
        }
        // Resized on the GPU (the cutout studio's small render): drawn whole, then scaled.
        let full = scoped(device, "export render", || {
            let full = output_texture_as(device, out_w, out_h, format);
            let sensor = denoised.as_ref().map_or(&image.sensor_view, |d| &d.view);
            let bind = self.adjust_bind(
                device,
                sensor,
                &image.sensor_view,
                &image.brushes.view,
                &self.export_buffer,
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("export encoder"),
            });
            draw_fullscreen(
                &mut encoder,
                "export adjust pass",
                pipeline,
                &bind,
                &full.create_view(&Default::default()),
            );
            queue.submit([encoder.finish()]);
            full
        })?;

        let (rx, ry, rw, rh) = rect;

        if let Some((tw, th)) = target_size {
            if (tw < rw || th < rh) && tw > 0 && th > 0 {
                let target_tex = output_texture_as(device, tw, th, format);
                let target_view = target_tex.create_view(&Default::default());
                let full_view = full.create_view(&Default::default());

                let crop_norm = [
                    rx as f32 / out_w as f32,
                    ry as f32 / out_h as f32,
                    rw as f32 / out_w as f32,
                    rh as f32 / out_h as f32,
                ];
                let uniforms = ResizeUniforms {
                    crop: crop_norm,
                    dest_size: [tw as f32, th as f32, 0.0, 0.0],
                };
                queue.write_buffer(&self.resize_buffer, 0, bytemuck::bytes_of(&uniforms));

                let resize_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("resize bind"),
                    layout: &self.resize_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&full_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.resize_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.resize_buffer.as_entire_binding(),
                        },
                    ],
                });

                let r_pipeline = if deep {
                    &self.deep_resize_pipeline
                } else {
                    &self.resize_pipeline
                };

                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("resize encoder"),
                });
                draw_fullscreen(
                    &mut encoder,
                    "export gpu resize",
                    r_pipeline,
                    &resize_bind,
                    &target_view,
                );
                queue.submit([encoder.finish()]);

                let pixels = read_back(
                    device,
                    queue,
                    &target_tex,
                    (0, 0, tw, th),
                    texel,
                    self.export_band_bytes(device),
                )?;
                return Ok((tw, th, pixels));
            }
        }

        let pixels = read_back(
            device,
            queue,
            &full,
            rect,
            texel,
            self.export_band_bytes(device),
        )?;
        Ok((rw, rh, pixels))
    }

    /// The export's `rect` of the output, drawn and read back one band of rows at a time
    /// through a single band-sized target, so the GPU never holds more of the frame than
    /// a band (`export_band_bytes`): a 100 MP 16-bit export held 800 MB drawn whole. Every
    /// pixel the adjust pass draws depends on its own position and the sensor alone, never
    /// on its neighbours in the target, so bands meet without seams; the shader is told
    /// where each band sits (`split.yz`). The uniforms are the export's, written already.
    #[allow(clippy::too_many_arguments)]
    fn render_bands(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &LoadedImage,
        sensor: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        format: wgpu::TextureFormat,
        rect: (u32, u32, u32, u32),
        texel: u32,
    ) -> Result<Vec<u8>> {
        let (rw, rh) = (rect.2, rect.3);
        let mut out = Vec::new();
        out.try_reserve_exact(rw as usize * rh as usize * texel as usize)
            .map_err(|_| anyhow::anyhow!("no memory for a {rw}×{rh} export"))?;
        self.render_bands_into(
            device,
            queue,
            image,
            sensor,
            pipeline,
            format,
            rect,
            texel,
            &mut |_, band| out.extend_from_slice(band),
        )?;
        Ok(out)
    }

    /// [`Self::render_bands`], each band handed to `sink` (its first row in the rect, and
    /// its tightly packed rows) as it is read back rather than gathered into one frame.
    #[allow(clippy::too_many_arguments)]
    fn render_bands_into(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &LoadedImage,
        sensor: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        format: wgpu::TextureFormat,
        (rx, ry, rw, rh): (u32, u32, u32, u32),
        texel: u32,
        sink: &mut dyn FnMut(u32, &[u8]),
    ) -> Result<()> {
        let band_bytes = self.export_band_bytes(device);
        let stride = crate::export::padded_row_bytes(rw, texel) as u64;
        let band_rows = (band_bytes / stride)
            .clamp(1, rh.max(1) as u64)
            .min(device.limits().max_texture_dimension_2d as u64) as u32;
        let band = scoped(device, "export render", || {
            output_texture_as(device, rw, band_rows, format)
        })?;
        let band_view = band.create_view(&Default::default());
        let bind = self.adjust_bind(
            device,
            sensor,
            &image.sensor_view,
            &image.brushes.view,
            &self.export_buffer,
        );
        let origin_at = std::mem::offset_of!(AdjustUniforms, split) as u64 + 4;
        let mut y = 0;
        while y < rh {
            let rows = band_rows.min(rh - y);
            let origin = [rx as f32, (ry + y) as f32];
            queue.write_buffer(
                &self.export_buffer,
                origin_at,
                bytemuck::cast_slice(&origin),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("export band encoder"),
            });
            draw_fullscreen(&mut encoder, "export band", pipeline, &bind, &band_view);
            queue.submit([encoder.finish()]);
            let pixels = read_back(device, queue, &band, (0, 0, rw, rows), texel, band_bytes)?;
            sink(y, &pixels);
            y += rows;
        }
        Ok(())
    }

    /// Choose the preview resolution: 1/2^level of full, clamped to what the sensor mip
    /// chain has. The caller picks the coarsest level that is still at least as dense as
    /// the screen, so the preview is never softer than the display.
    ///
    /// Returns true if the preview texture was replaced, which invalidates any registered
    /// view — the same contract as `set_geometry`.
    pub fn set_preview_level(&mut self, device: &wgpu::Device, level: u32) -> bool {
        let Some(image) = &mut self.image else {
            return false;
        };
        let level = level.min(image.mip_levels - 1);
        self.preview_level = level;
        if image.preview_level == level {
            return false;
        }
        image.preview_level = level;
        self.replace_output(device);
        true
    }

    /// Reallocate the preview texture for the current geometry and preview level.
    fn replace_output(&mut self, device: &wgpu::Device) {
        let Some(image) = &mut self.image else {
            return;
        };
        let (w, h) = image.preview_size();
        match scoped(device, "the preview texture", || {
            output_texture(device, w, h)
        }) {
            Ok(texture) => {
                image.output_view = texture.create_view(&Default::default());
                image.output_texture = texture;
                image.hist_bind = std::sync::OnceLock::new();
            }
            // Preview-sized, so this is a device in real trouble. The old texture is still
            // valid to draw and show, which beats a panic mid-frame.
            Err(e) => log::error!("{e:#}; keeping the previous preview"),
        }
        // Whatever readback is in flight describes a texture that no longer exists.
        self.hist_epoch = self.hist_epoch.wrapping_add(1);
        self.histogram = Histogram::default();
        self.sync_overlay(device);
    }

    /// Apply orientation, resizing the output texture only when right-angle turns actually
    /// change its dimensions.
    ///
    /// Returns true if the texture was replaced, which invalidates any registered view.
    /// Straightening and mirroring never resize, so they never cost an allocation, and the
    /// crop is not applied here at all — see `params::Crop`.
    pub fn set_geometry(&mut self, device: &wgpu::Device, geometry: Geometry) -> bool {
        let Some(image) = &mut self.image else {
            return false;
        };
        if image.geometry == geometry {
            return false;
        }

        let (w, h) = geometry.output_size(image.sensor_width, image.sensor_height);
        let resized = w != image.width || h != image.height;
        image.geometry = geometry;

        if !resized {
            return false;
        }

        image.width = w;
        image.height = h;
        self.replace_output(device);
        true
    }

    /// Re-run the adjustment chain into the preview. This is the per-frame path and the
    /// per-slider-tick path — it writes one uniform buffer and submits one fullscreen draw,
    /// at the preview's resolution rather than the sensor's (see `set_preview_level`).
    ///
    /// If `split_x` is `Some(fraction)` (0.0..=1.0), the shader renders a before/after
    /// split: the left side (`< fraction`) shows the unadjusted raw baseline, the right side
    /// the active adjustments. The divider line is the UI's to draw.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        split_x: Option<f32>,
    ) {
        // Brushes first: a brushed mask's noise reduction reads them.
        if let Some(image) = &mut self.image {
            image.brushes.sync(queue, &brush_layers(stack));
        }
        self.sync_denoise(device, queue, stack);
        self.sync_fills(device, queue, stack);
        self.sync_look(queue, stack);
        let Some(image) = &self.image else { return };
        let target = image.preview_size();
        let level = image.preview_level;
        let out_size = (image.width, image.height);
        let base = image.denoised.as_ref().map_or(0, |(d, _)| d.base);
        self.write_uniforms(
            queue,
            stack,
            split_x,
            (out_size, target, level, base),
            false,
            true,
            if self.screen_table_loaded {
                // The table's input: Rec.2020, encoded with the sRGB curve.
                Target {
                    out: crate::gamut::Screen {
                        from_srgb: crate::gamut::convert(
                            &crate::gamut::SRGB,
                            &crate::gamut::REC2020,
                        ),
                        gamma: None,
                    },
                    proof: self.proof,
                    screen_table: true,
                    hdr_white: None,
                }
            } else {
                Target {
                    out: self.screen,
                    proof: self.proof,
                    screen_table: false,
                    hdr_white: None,
                }
            },
        );
        let Some(image) = &self.image else { return };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("adjust encoder"),
        });
        draw_fullscreen(
            &mut encoder,
            "adjust pass",
            &self.adjust_pipeline,
            image
                .denoised
                .as_ref()
                .map_or(&image.adjust_bind, |(_, bind)| bind),
            &image.output_view,
        );

        // Scopes describe what the photograph will be, so they measure inside the crop — a
        // histogram of pixels the user has framed out is actively misleading — and only
        // the "after" side of a split.
        let (ox, oy, w, h) = stack.geometry.crop.to_pixels(target.0, target.1);
        let (ox, w) = match split_x {
            Some(split) => {
                let divider = (split.clamp(0.0, 1.0) * target.0 as f32) as u32;
                let start = ox.max(divider).min(ox + w);
                if start < ox + w {
                    (start, ox + w - start)
                } else {
                    (ox, w)
                }
            }
            None => (ox, w),
        };
        let region = [ox, oy, w, h];

        // Only queue a new histogram when the previous one has been collected. Otherwise a
        // fast drag would pile up map requests against a buffer that is already mapped;
        // the frame is owed a measurement instead, paid by `poll_histogram`.
        let measuring = self.hist_state.is_idle();
        // While a mask is washed red on the preview, the preview is not the photo: the
        // scopes keep their last true reading until the wash goes.
        let washed = self.mask_overlay.is_some() && stack.local.enabled;
        if washed {
            self.hist_owed = None;
        } else if measuring {
            self.hist_owed = None;
            self.encode_histogram(device, queue, &mut encoder, region, level);
        } else {
            self.hist_owed = Some((region, level));
        }

        if let Some((_, view, bind)) = self.image.as_ref().and_then(|im| im.overlay.as_ref()) {
            draw_fullscreen(
                &mut encoder,
                "clip overlay pass",
                &self.clip_pipeline,
                bind,
                view,
            );
        }

        queue.submit([encoder.finish()]);
        if measuring {
            self.start_histogram_readback();
        }
    }

    /// Everything a pass needs, into the one uniform buffer, plus the curve LUT if it
    /// changed. `out_size` is the full-resolution frame after right-angle turns, `target`
    /// the render target's size and `level` the sensor mip to sample.
    #[allow(clippy::too_many_arguments)]
    fn write_uniforms(
        &mut self,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        split_x: Option<f32>,
        (out_size, target, level, base): ((u32, u32), (u32, u32), u32, u32),
        for_export: bool,
        dither: bool,
        ends: Target,
    ) {
        let Some(image) = &self.image else { return };
        let lens = LensModel::new(&stack.lens.params);
        let warp = image.warp_for(stack);
        let spots = spot_uniforms(
            stack,
            (image.sensor_width, image.sensor_height),
            image.mip_levels,
            &image.fills,
        );
        let masks = mask_uniforms(
            stack,
            (image.sensor_width, image.sensor_height),
            self.mask_overlay,
        );

        let master = stack.tone_curve.enabled.then_some(&stack.tone_curve.params);
        let rgb = stack.rgb_curves.enabled.then_some(&stack.rgb_curves.params);
        if (master.is_some() || rgb.is_some())
            && (self.cached_curve.as_ref() != master || self.cached_rgb_curves.as_ref() != rgb)
        {
            let lut = build_lut_4channel(master.map(Vec::as_slice), rgb);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.lut_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&lut),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(LUT_SIZE as u32 * 4),
                    rows_per_image: Some(4),
                },
                wgpu::Extent3d {
                    width: LUT_SIZE as u32,
                    height: 4,
                    depth_or_array_layers: 1,
                },
            );
            self.cached_curve = master.cloned();
            self.cached_rgb_curves = rgb.cloned();
        }

        let g = stack.geometry;
        let wb = stack.white_balance.params;
        // Camera → working space, and working → output (W-09).
        let working = stack.working_space.primaries();
        let m = match stack.working_space {
            crate::gamut::WorkingSpace::Srgb => image.cam_to_srgb,
            crate::gamut::WorkingSpace::Rec2020 => crate::gamut::compose(
                &crate::gamut::convert(&crate::gamut::SRGB, &working),
                match stack.rendering {
                    crate::params::Rendering::V1 => &image.cam_profiled,
                    crate::params::Rendering::V2 => &image.cam_profiled_v2,
                },
            ),
        };
        // Working → the proof's space if there is one, else the output's; then, proofing,
        // on to the screen.
        use crate::gamut::{compose, convert, invert3, Screen, SRGB};
        let first = ends.proof.map_or(ends.out, |p| p.space);
        let to_out = compose(&first.from_srgb, &convert(&working, &SRGB));
        let converts = stack.working_space != crate::gamut::WorkingSpace::Srgb
            || first.from_srgb != Screen::SRGB.from_srgb;
        let proof_to_screen = compose(&ends.out.from_srgb, &invert3(&first.from_srgb));
        let luma_w = crate::gamut::luminance(&working);
        let f = stack.filmic.params;
        let cb = stack.color_balance.params;

        queue.write_buffer(
            if for_export {
                &self.export_buffer
            } else {
                &self.adjust_buffer
            },
            0,
            bytemuck::bytes_of(&AdjustUniforms {
                cam0: [m[0][0], m[0][1], m[0][2], 0.0],
                cam1: [m[1][0], m[1][1], m[1][2], 0.0],
                cam2: [m[2][0], m[2][1], m[2][2], 0.0],
                wb: [
                    image.wb[0] * wb.tint_r,
                    image.wb[1],
                    image.wb[2] * wb.tint_b,
                    0.0,
                ],
                exposure: [
                    2f32.powf(stack.exposure.params.ev),
                    stack.exposure.params.black,
                    stack.exposure.params.rolloff,
                    0.75, // tau: knee threshold
                ],
                filmic: [f.white_ev, f.black_ev, f.latitude, f.contrast],
                lift: cb.lift.to_array(),
                gamma: cb.gamma.to_array(),
                gain: cb.gain.to_array(),
                offset: cb.offset.to_array(),
                flags: [
                    // The look's flag only while its table holds it.
                    (stack.bitmask() & if self.look_loaded { !0 } else { !LOOK_FLAG })
                        | if self.nan_paint { NAN_PAINT } else { 0 },
                    (stack.curve_domain == crate::params::CurveDomain::Display) as u32,
                    dither as u32,
                    stack.look.params.amount.clamp(0.0, 1.0).to_bits(),
                ],
                sizes: [
                    out_size.0 as f32,
                    out_size.1 as f32,
                    image.sensor_width as f32,
                    image.sensor_height as f32,
                ],
                // The INVERSE straighten: the shader maps output back to sensor.
                orient: [
                    (-g.straighten_deg.to_radians()).cos(),
                    (-g.straighten_deg.to_radians()).sin(),
                    if g.mirror_h { -1.0 } else { 1.0 },
                    if g.mirror_v { -1.0 } else { 1.0 },
                ],
                turns: [g.quarter_turns as u32, 0, 0, 0],
                split: [split_x.unwrap_or(-1.0), 0.0, 0.0, 0.0],
                as_shot_wb: [image.wb[0], image.wb[1], image.wb[2], 0.0],
                tonal: [
                    stack.tonal.params.highlights,
                    stack.tonal.params.shadows,
                    stack.tonal.params.whites,
                    stack.tonal.params.blacks,
                ],
                vibrance: [
                    stack.vibrance.params.vibrance,
                    stack.vibrance.params.saturation,
                    0.0,
                    0.0,
                ],
                vignette: [
                    stack.vignette.params.amount,
                    stack.vignette.params.radius,
                    0.0,
                    0.0,
                ],
                sharpen: [stack.sharpen.params.amount, 0.0, 0.0, 0.0],
                dest: [target.0 as f32, target.1 as f32, level as f32, base as f32],
                detail: [
                    stack.clarity.params.clarity,
                    stack.clarity.params.texture,
                    stack.dehaze.params.amount,
                    0.0,
                ],
                eq_red: [
                    stack.color_eq.params.red.hue,
                    stack.color_eq.params.red.saturation,
                    stack.color_eq.params.red.luminance,
                    0.0,
                ],
                eq_orange: [
                    stack.color_eq.params.orange.hue,
                    stack.color_eq.params.orange.saturation,
                    stack.color_eq.params.orange.luminance,
                    0.0,
                ],
                eq_yellow: [
                    stack.color_eq.params.yellow.hue,
                    stack.color_eq.params.yellow.saturation,
                    stack.color_eq.params.yellow.luminance,
                    0.0,
                ],
                eq_green: [
                    stack.color_eq.params.green.hue,
                    stack.color_eq.params.green.saturation,
                    stack.color_eq.params.green.luminance,
                    0.0,
                ],
                eq_aqua: [
                    stack.color_eq.params.aqua.hue,
                    stack.color_eq.params.aqua.saturation,
                    stack.color_eq.params.aqua.luminance,
                    0.0,
                ],
                eq_blue: [
                    stack.color_eq.params.blue.hue,
                    stack.color_eq.params.blue.saturation,
                    stack.color_eq.params.blue.luminance,
                    0.0,
                ],
                eq_purple: [
                    stack.color_eq.params.purple.hue,
                    stack.color_eq.params.purple.saturation,
                    stack.color_eq.params.purple.luminance,
                    0.0,
                ],
                eq_magenta: [
                    stack.color_eq.params.magenta.hue,
                    stack.color_eq.params.magenta.saturation,
                    stack.color_eq.params.magenta.luminance,
                    0.0,
                ],
                frame: {
                    let c = stack.geometry.crop.sanitised();
                    [c.x, c.y, c.width, c.height]
                },
                lens: [lens.k, lens.zoom, lens.ca_red, lens.ca_blue],
                lens_b: [lens.vignetting_stops, 0.0, 0.0, 0.0],
                warp: {
                    let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                    let planes = warp.map_or([identity; 3], |w| w.planes);
                    let half = |p: usize, h: usize| {
                        let c = planes[p];
                        [c[h * 4], c[h * 4 + 1], c[h * 4 + 2], c[h * 4 + 3]]
                    };
                    [
                        half(0, 0),
                        half(0, 1),
                        half(1, 0),
                        half(1, 1),
                        half(2, 0),
                        half(2, 1),
                    ]
                },
                warp_c: warp.map_or([0.0, 0.0, 1.0, 0.0], |w| {
                    [w.centre[0], w.centre[1], w.m, 1.0]
                }),
                tca_r: warp
                    .and_then(|w| w.tca)
                    .map_or([1.0, 0.0, 0.0, 0.0], |t| [t[0][0], t[0][1], t[0][2], 1.0]),
                tca_b: warp
                    .and_then(|w| w.tca)
                    .map_or([1.0, 0.0, 0.0, 0.0], |t| [t[1][0], t[1][1], t[1][2], 1.0]),
                warp_vig: warp
                    .and_then(|w| w.vignetting)
                    .map_or([0.0; 4], |(k, unit)| [k[0], k[1], k[2], unit]),
                masks: masks.0,
                mask_info: [
                    masks.1,
                    if for_export { 0 } else { masks.2 },
                    u32::from(self.dust_view && !for_export),
                    spots.1,
                ],
                spots: spots.0,
                persp: match stack.geometry.perspective(out_size) {
                    Some(p) => [
                        [p.m[0][0], p.m[0][1], p.m[0][2], p.unit],
                        [p.m[1][0], p.m[1][1], p.m[1][2], 1.0],
                        [p.m[2][0], p.m[2][1], p.m[2][2], 0.0],
                    ],
                    None => [[0.0; 4]; 3],
                },
                luma_w: [
                    luma_w[0],
                    luma_w[1],
                    luma_w[2],
                    f32::from(stack.working_space == crate::gamut::WorkingSpace::Rec2020),
                ],
                out_m: [
                    [
                        to_out[0][0],
                        to_out[0][1],
                        to_out[0][2],
                        ends.out.gamma.unwrap_or(0.0),
                    ],
                    [
                        to_out[1][0],
                        to_out[1][1],
                        to_out[1][2],
                        f32::from(converts),
                    ],
                    [
                        to_out[2][0],
                        to_out[2][1],
                        to_out[2][2],
                        f32::from(ends.screen_table),
                    ],
                ],
                proof_m: {
                    let m = proof_to_screen;
                    let table = self.proof_table_loaded;
                    let (on, warn) = ends.proof.map_or((0.0, 0.0), |p| {
                        (
                            if p.printer && table { 2.0 } else { 1.0 },
                            f32::from(p.warn),
                        )
                    });
                    [
                        [m[0][0], m[0][1], m[0][2], on],
                        [m[1][0], m[1][1], m[1][2], warn],
                        [m[2][0], m[2][1], m[2][2], first.gamma.unwrap_or(0.0)],
                    ]
                },
                hdr: match ends.hdr_white {
                    // The output's luminance: sRGB's, of the output's RGB taken back to sRGB.
                    Some(white) => {
                        let back = invert3(&ends.out.from_srgb);
                        let y = crate::gamut::luminance(&SRGB);
                        let w = |j: usize| (0..3).map(|i| y[i] * back[i][j]).sum::<f32>();
                        [w(0), w(1), w(2), white]
                    }
                    None => [0.0; 4],
                },
            }),
        );
    }

    /// The most recent histogram. Never blocks: if the GPU has not finished, the previous
    /// one is returned and the plot is one interaction stale, which is invisible.
    pub fn histogram(&self) -> &Histogram {
        &self.histogram
    }

    /// Record the histogram compute pass for `region` of the current output.
    fn encode_histogram(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        region: [u32; 4],
        level: u32,
    ) {
        let Some(image) = &self.image else { return };
        let [_, _, w, h] = region;
        // A preview at level L already has 4^L fewer pixels; keep roughly the same sample
        // count rather than striding over what is left.
        let stride = (HISTOGRAM_STRIDE >> level).max(1);
        queue.write_buffer(
            &self.hist_params,
            0,
            bytemuck::bytes_of(&HistogramParams {
                region,
                stride: [stride, 0, 0, 0],
            }),
        );
        let bind = image.hist_bind.get_or_init(|| {
            device.make_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("histogram bind"),
                layout: &self.hist_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&image.output_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.hist_bins.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.hist_params.as_entire_binding(),
                    },
                ],
            })
        });

        encoder.clear_buffer(&self.hist_bins, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("histogram pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.hist_pipeline);
            pass.set_bind_group(0, bind, &[]);
            let groups_x = w.div_ceil(stride).div_ceil(16).max(1);
            let groups_y = h.div_ceil(stride).div_ceil(16).max(1);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        encoder.copy_buffer_to_buffer(&self.hist_bins, 0, &self.hist_staging, 0, BUFFER_BYTES);
    }

    /// After the histogram pass is submitted: map its result for `poll_histogram`.
    fn start_histogram_readback(&mut self) {
        self.hist_inflight_epoch = self.hist_epoch;
        self.hist_state.set_pending();
        let state = self.hist_state.clone();
        self.hist_staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                // A failed map leaves the previous histogram on screen, which is a better
                // outcome than a panic in a scope widget.
                if result.is_ok() {
                    state.set_ready();
                } else {
                    state.set_idle();
                }
            });
    }

    /// A histogram is in flight or owed: the caller should come back next frame even if
    /// nothing else changes, or the scopes will sit on a stale reading.
    pub fn histogram_busy(&self) -> bool {
        !self.hist_state.is_idle() || self.hist_owed.is_some()
    }

    /// Collect a finished histogram and, once the readback is free, measure the frame a
    /// render was owed. Cheap, and safe to call every frame.
    pub fn poll_histogram(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Drives wgpu's map callbacks; without this the readback never completes.
        let _ = device.poll(wgpu::PollType::Poll);
        self.collect_histogram();
        if self.hist_state.is_idle() {
            if let Some((region, level)) = self.hist_owed.take() {
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("histogram encoder"),
                });
                self.encode_histogram(device, queue, &mut encoder, region, level);
                queue.submit([encoder.finish()]);
                self.start_histogram_readback();
            }
        }
    }

    fn collect_histogram(&mut self) {
        if !self.hist_state.is_ready() {
            return;
        }
        if self.hist_inflight_epoch == self.hist_epoch {
            let slice = self.hist_staging.slice(..);
            if let Ok(view) = slice.get_mapped_range() {
                let raw: &[u32] = bytemuck::cast_slice(&view);
                self.histogram = Histogram::from_bins(raw);
            }
        }
        // Whether it was accepted or discarded, the buffer must be released before the
        // next dispatch can reuse it.
        self.hist_staging.unmap();
        self.hist_state.set_idle();
    }
}

/// The largest readback buffer export allocates, whatever the device would allow.
///
/// A 24 MP frame is ~98 MB of RGBA and a 100 MP one ~400 MB, while `downlevel_defaults`
/// caps a buffer at 256 MiB — so one buffer per export crashed every large export. Bands
/// of this size keep the GPU-side cost of a readback flat at any resolution, and it is
/// still few enough round trips (four at 100 MP) that the sync cost doesn't show.
pub(crate) const MAX_BAND_BYTES: u64 = 64 << 20;

/// How long a readback waits on the GPU before calling it lost. Generous — a 100 MP render
/// on a software rasteriser takes seconds — but finite: a device that has gone away must
/// turn into an error, not a UI thread blocked forever.
const GPU_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Validation and out-of-memory errors from a block of GPU work, returned as an `Err`.
///
/// wgpu's default for an error nothing captured is to panic, which on a large export means
/// losing the app — and with it any unsaved edit — to an allocation the device refused.
struct ErrorScopes {
    // Declared innermost first: an early return drops the fields in this order, and wgpu
    // panics unless scopes pop in the reverse of the order they were pushed.
    validation: wgpu::ErrorScopeGuard,
    oom: wgpu::ErrorScopeGuard,
}

impl ErrorScopes {
    fn push(device: &wgpu::Device) -> Self {
        let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        Self { oom, validation }
    }

    /// Scopes pop in reverse order. On native wgpu the result is ready as soon as the pop
    /// returns, so it is read without an executor; a backend that could only answer later
    /// is treated as having reported nothing.
    fn pop(self, what: &str) -> Result<()> {
        let validation = ready_now(self.validation.pop()).flatten();
        let oom = ready_now(self.oom.pop()).flatten();
        match validation.or(oom) {
            Some(wgpu::Error::OutOfMemory { .. }) => bail!("{what}: the GPU is out of memory"),
            Some(e) => bail!("{what}: {e}"),
            None => Ok(()),
        }
    }
}

/// `f`, with any GPU error it raises returned as an `Err` instead of a panic.
fn scoped<T>(device: &wgpu::Device, what: &str, f: impl FnOnce() -> T) -> Result<T> {
    let scopes = ErrorScopes::push(device);
    let value = f();
    scopes.pop(what)?;
    Ok(value)
}

/// Poll a future once. For wgpu futures that native backends resolve immediately.
fn ready_now<F: std::future::Future>(future: F) -> Option<F::Output> {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(value) => Some(value),
        std::task::Poll::Pending => None,
    }
}

/// Block until `index` has executed, or fail after `GPU_TIMEOUT`.
fn wait_for(device: &wgpu::Device, index: wgpu::SubmissionIndex) -> Result<()> {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: Some(GPU_TIMEOUT),
        })
        .map_err(|e| anyhow::anyhow!("the GPU did not finish: {e}"))?;
    Ok(())
}

/// Wait for a `map_async` callback. The submission it waited on having finished is not
/// enough: the device is shared (the batch exporter, the loader, tests running in
/// parallel), and whichever thread polls next is the one that runs the callback, possibly
/// a moment after this thread's own poll returned. So poll until it arrives.
fn map_result(
    device: &wgpu::Device,
    rx: &std::sync::mpsc::Receiver<std::result::Result<(), wgpu::BufferAsyncError>>,
) -> Result<()> {
    let deadline = std::time::Instant::now() + GPU_TIMEOUT;
    loop {
        match rx.try_recv() {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => bail!("export readback: {e}"),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                bail!("export readback: the mapping was dropped")
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if std::time::Instant::now() > deadline {
                    bail!("export readback: the GPU never mapped the buffer");
                }
                let _ = device.poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_millis(10)),
                });
            }
        }
    }
}

/// Copy `(x, y, width, height)` of an RGBA8 texture back to the CPU, tightly packed.
///
/// In horizontal bands of at most `band_bytes`, through one reused `MAP_READ` buffer, so no
/// buffer is ever larger than the band however large the image (see `MAX_BAND_BYTES`). The
/// texture is rendered once beforehand; only the copy is split.
pub(crate) fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    (ox, oy, width, height): (u32, u32, u32, u32),
    texel: u32,
    band_bytes: u64,
) -> Result<Vec<u8>> {
    read_back_level(
        device,
        queue,
        texture,
        0,
        (ox, oy, width, height),
        texel,
        band_bytes,
    )
}

/// [`read_back`] from mip level `mip`, in that level's coordinates.
pub(crate) fn read_back_level(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    mip: u32,
    (ox, oy, width, height): (u32, u32, u32, u32),
    texel: u32,
    band_bytes: u64,
) -> Result<Vec<u8>> {
    let stride = crate::export::padded_row_bytes(width, texel);
    let row = (width * texel) as usize;
    // At least one row, whatever the limit: a row is never more than 64 KiB × 4.
    let band_rows = (band_bytes / stride as u64).clamp(1, height.max(1) as u64) as u32;

    let mut out = Vec::new();
    out.try_reserve_exact(row * height as usize)
        .map_err(|_| anyhow::anyhow!("no memory for a {width}×{height} export"))?;

    let buffer = scoped(device, "export readback buffer", || {
        device.make_buffer(&wgpu::BufferDescriptor {
            label: Some("export readback"),
            size: stride as u64 * band_rows as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    })?;

    let mut y = 0;
    while y < height {
        let rows = band_rows.min(height - y);
        let scopes = ErrorScopes::push(device);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("export readback encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: mip,
                origin: wgpu::Origin3d {
                    x: ox,
                    y: oy + y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(rows),
                },
            },
            wgpu::Extent3d {
                width,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
        let index = queue.submit([encoder.finish()]);
        scopes.pop("export readback")?;

        let slice = buffer.slice(..stride as u64 * rows as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        wait_for(device, index)?;
        map_result(device, &rx)?;
        {
            let mapped = slice.get_mapped_range()?;
            for r in 0..rows as usize {
                let start = r * stride as usize;
                out.extend_from_slice(&mapped[start..start + row]);
            }
        }
        buffer.unmap();
        y += rows;
    }
    Ok(out)
}

/// Sensor texels and the grid they form (width, height).
type Overview = (Vec<[f32; 4]>, (u32, u32));

/// The heal spots, packed for the shader, and how many.
fn spot_uniforms(
    stack: &PhotoStack,
    sensor: (u32, u32),
    mip_levels: u32,
    fills: &[Option<Fill>],
) -> ([[f32; 4]; 64], u32) {
    let mut out = [[0.0f32; 4]; 64];
    if !stack.heal.enabled {
        return (out, 0);
    }
    let (w, h) = (sensor.0 as f32, sensor.1 as f32);
    let long = w.max(h);
    let mut n = 0;
    let mut removes = 0;
    for spot in stack.heal.params.spots.iter().take(crate::heal::MAX_SPOTS) {
        let r = (spot.radius * long).max(1.0);
        if spot.mode == crate::heal::Mode::Remove {
            let slot = removes;
            removes += 1;
            // Mode 2 + its slot, and the window it was filled from. A spot that could not
            // be filled (or found no slot) is healed from its source instead, below.
            if let Some(Some(fill)) = fills.get(slot) {
                out[n * 2] = [
                    spot.dst[0] * w,
                    spot.dst[1] * h,
                    fill.origin[0],
                    fill.origin[1],
                ];
                out[n * 2 + 1] = [
                    r,
                    spot.feather.clamp(0.01, 1.0),
                    2.0 + slot as f32,
                    fill.scale,
                ];
                n += 1;
                continue;
            }
        }
        // The ring is measured where a texel is about half the spot's radius.
        let level = (r * 0.5)
            .max(1.0)
            .log2()
            .clamp(0.0, mip_levels.saturating_sub(1) as f32);
        out[n * 2] = [
            spot.dst[0] * w,
            spot.dst[1] * h,
            spot.src[0] * w,
            spot.src[1] * h,
        ];
        out[n * 2 + 1] = [
            r,
            spot.feather.clamp(0.01, 1.0),
            match spot.mode {
                crate::heal::Mode::Heal | crate::heal::Mode::Remove => 0.0,
                crate::heal::Mode::Clone => 1.0,
            },
            level,
        ];
        n += 1;
    }
    (out, n as u32)
}

/// The masks that are on, packed for the shader, how many, and 1 + the shown one's slot.
fn mask_uniforms(
    stack: &PhotoStack,
    sensor: (u32, u32),
    shown: Option<usize>,
) -> ([[f32; 4]; 40], u32, u32) {
    use crate::local::{RangeMask, Shape};
    let mut out = [[0.0f32; 4]; 40];
    let mut n = 0u32;
    let mut overlay = 0u32;
    if !stack.local.enabled {
        return (out, 0, 0);
    }
    let (w, h) = (sensor.0 as f32, sensor.1 as f32);
    let long = w.max(h);
    let masks = stack
        .local
        .params
        .masks
        .iter()
        .enumerate()
        .filter(|(_, m)| m.on);
    let mut brush = 0.0f32;
    for (i, m) in masks.take(crate::local::MAX_MASKS) {
        let slot = n as usize * 5;
        // Kind (0 gradient, 1 radial, 2 brush), plus 4 when inverted.
        let invert = if m.invert { 4.0 } else { 0.0 };
        match m.shape {
            Shape::Brush { .. } => {
                out[slot] = [brush, 0.0, 0.0, 0.0];
                out[slot + 1] = [0.0, 0.0, 0.0, 2.0 + invert];
                brush += 1.0;
            }
            Shape::Linear { from, to } => {
                out[slot] = [from[0] * w, from[1] * h, to[0] * w, to[1] * h];
                out[slot + 1] = [0.0, 0.0, 0.0, invert];
            }
            Shape::Radial {
                centre,
                radius,
                angle,
                feather,
            } => {
                let (sin, cos) = angle.to_radians().sin_cos();
                out[slot] = [
                    centre[0] * w,
                    centre[1] * h,
                    (radius[0] * long).max(1.0),
                    (radius[1] * long).max(1.0),
                ];
                out[slot + 1] = [cos, sin, feather.clamp(0.01, 1.0), 1.0 + invert];
            }
        }
        let a = &m.adjust;
        out[slot + 2] = [a.exposure, a.temperature, a.tint, a.contrast];
        out[slot + 3] = [a.saturation, a.clarity, a.sharpness, a.denoise];
        match m.range {
            RangeMask::None => {
                out[slot + 4] = [0.0, 0.0, 0.0, 0.0];
            }
            RangeMask::Luminance { range, feather } => {
                out[slot + 4] = [1.0, range[0], range[1], feather.clamp(0.001, 1.0)];
            }
            RangeMask::Color {
                hue,
                spread,
                feather,
            } => {
                out[slot + 4] = [2.0, hue, spread, feather.clamp(0.001, 1.0)];
            }
        }
        if shown == Some(i) {
            overlay = n + 1;
        }
        n += 1;
    }
    (out, n, overlay)
}

/// `denoise.wgsl`'s parameters for `params`, level left for the caller: see its `Params`.
/// The masks that ask for noise reduction (W-01), packed for `denoise.wgsl`, and their
/// brush strokes' lengths, so a stroke laid down is noticed as a change.
#[derive(Debug, Clone, PartialEq, Default)]
struct LocalDenoise {
    masks: [[f32; 4]; 32],
    count: u32,
    dabs: Vec<usize>,
}

impl LocalDenoise {
    fn of(stack: &PhotoStack, sensor: (u32, u32)) -> Self {
        let (all, n, _) = mask_uniforms(stack, sensor, None);
        let mut out = Self::default();
        for i in 0..n as usize {
            if all[i * 5 + 3][3] > 0.0 {
                let to = out.count as usize * 4;
                out.masks[to..to + 4].copy_from_slice(&all[i * 5..i * 5 + 4]);
                out.count += 1;
            }
        }
        if out.count > 0 {
            out.dabs = brush_layers(stack).iter().map(|d| d.len()).collect();
        }
        out
    }
}

/// `denoise.wgsl`'s `Params`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DenoiseUniforms {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    masks: [[f32; 4]; 32],
}

/// What noise reduction a stack asks for: the module's settings (or none at all when it
/// is off), and the local masks'. `None` when nothing does.
fn denoise_wanted(stack: &PhotoStack, sensor: (u32, u32)) -> Option<(Denoise, LocalDenoise)> {
    let global = stack.denoise.enabled && !stack.denoise.params.is_neutral();
    let local = LocalDenoise::of(stack, sensor);
    if !global && local.count == 0 {
        return None;
    }
    let params = if global {
        stack.denoise.params
    } else {
        Denoise {
            luminance: 0.0,
            color: 0.0,
            ..Denoise::default()
        }
    };
    Some((params, local))
}

fn denoise_uniforms(
    params: &Denoise,
    mip_levels: u32,
    local: &LocalDenoise,
    sensor: (u32, u32),
) -> DenoiseUniforms {
    let [a0, a1, a2, a3, b0, b1, b2, b3] = denoise_params(params, mip_levels);
    DenoiseUniforms {
        a: [a0, a1, a2, a3],
        b: [b0, b1, b2, b3],
        c: [local.count as f32, sensor.0 as f32, sensor.1 as f32, 0.0],
        masks: local.masks,
    }
}

fn denoise_params(params: &Denoise, mip_levels: u32) -> [f32; 8] {
    let unit = |v: f32| {
        if v.is_finite() {
            (v / 100.0).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    [
        0.0,
        // At 100, noise of 6% of white at full scale, as a high-ISO file has at mid grey.
        unit(params.luminance) * 0.06,
        // Detail 0 averages steps up to three noise sigmas; 100 only up to one.
        3.0 - 2.0 * unit(params.detail),
        unit(params.color),
        // One to three levels coarser: 2x2 to 8x8 photosites per colour tap.
        (1.0 + 2.0 * unit(params.smoothness)).round(),
        mip_levels.saturating_sub(1) as f32,
        0.0,
        0.0,
    ]
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A baked 3D table into its texture.
fn write_table(queue: &wgpu::Queue, texture: &wgpu::Texture, table: &crate::proof::ProofTable) {
    let side = crate::proof::SIZE as u32;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&table.texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 8),
            rows_per_image: Some(side),
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: side,
        },
    );
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    bgl: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    fullscreen_pipeline_at(device, label, shader, "fs", bgl, format)
}

/// `fullscreen_pipeline` with a fragment entry point other than `fs`.
fn fullscreen_pipeline_at(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    entry: &str,
    bgl: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl Engine {
    /// RCD's four passes over a Bayer mosaic (`shaders/demosaic_rcd.wgsl`), into `target`.
    /// Its two scratch planes live only for this demosaic: kept, they would hold another
    /// 6 bytes a photosite for as long as the photo is open.
    #[allow(clippy::too_many_arguments)]
    fn demosaic_rcd(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        mosaic: &wgpu::TextureView,
        uniforms: &wgpu::Buffer,
        green: &wgpu::TextureView,
        target: &wgpu::TextureView,
        extent: wgpu::Extent3d,
    ) {
        let texture = |label, size, format, mips| {
            device.make_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        };
        let plane =
            |label, size, format| texture(label, size, format, 1).create_view(&Default::default());
        let dirs = plane("rcd directions", extent, RCD_DIRS_FORMAT);
        let opposite = plane("rcd opposite colour", extent, RCD_OPPOSITE_FORMAT);
        // What a pass does not read yet, since a pass may not read what it writes.
        let unused = plane(
            "rcd unused",
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            RCD_OPPOSITE_FORMAT,
        );
        // The highlight ratios: one texel a Bayer quad, and every level down to one texel.
        let quads = wgpu::Extent3d {
            width: (extent.width / 2).max(1),
            height: (extent.height / 2).max(1),
            depth_or_array_layers: 1,
        };
        let ratio_levels = 32 - quads.width.max(quads.height).leading_zeros();
        let ratios = texture("rcd highlight ratios", quads, SENSOR_FORMAT, ratio_levels);
        let ratio_level = |level: u32| {
            ratios.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let all_ratios = ratios.create_view(&Default::default());
        let bind = |label, reads: [&wgpu::TextureView; 4]| {
            device.make_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.rcd_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(mosaic),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(reads[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(reads[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(reads[2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(reads[3]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&self.rcd_sampler),
                    },
                ],
            })
        };

        let base = ratio_level(0);
        let ratio_bind = bind("rcd ratios pass", [&unused; 4]);
        draw_fullscreen(
            encoder,
            "rcd ratios pass",
            &self.rcd_pipelines[0],
            &ratio_bind,
            &base,
        );
        for level in 1..ratio_levels {
            let parent = ratio_level(level - 1);
            let mip = device.make_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rcd ratio downsample"),
                layout: &self.mip_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&parent),
                }],
            });
            draw_fullscreen(
                encoder,
                "rcd ratio downsample",
                &self.mip_pipeline,
                &mip,
                &ratio_level(level),
            );
        }

        let passes: [(&str, [&wgpu::TextureView; 4], &wgpu::TextureView); 4] = [
            (
                "rcd directions pass",
                [&unused, &unused, &unused, &unused],
                &dirs,
            ),
            ("rcd green pass", [&dirs, &unused, &unused, &unused], green),
            (
                "rcd opposite pass",
                [&dirs, green, &unused, &unused],
                &opposite,
            ),
            (
                "rcd final pass",
                [&dirs, green, &opposite, &all_ratios],
                target,
            ),
        ];
        for (i, (label, reads, writes)) in passes.into_iter().enumerate() {
            let bind = bind(label, reads);
            draw_fullscreen(encoder, label, &self.rcd_pipelines[i + 1], &bind, writes);
        }
    }
}

fn draw_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    pipeline: &wgpu::RenderPipeline,
    bind: &wgpu::BindGroup,
    target: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    // Three vertices, no vertex buffer — the shader derives an oversized triangle.
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod output_point_tests {
    use super::*;

    #[test]
    fn output_point_undoes_sensor_point_through_any_framing() {
        let sensor = (600u32, 400u32);
        for turns in 0..4 {
            for (mh, mv) in [(false, false), (true, false), (false, true)] {
                let mut stack = PhotoStack::default();
                let g = &mut stack.geometry;
                g.quarter_turns = turns;
                g.mirror_h = mh;
                g.mirror_v = mv;
                g.straighten_deg = 7.5;
                g.perspective_v = 35.0;
                g.perspective_h = -20.0;
                let out = (123.0, 77.0);
                let s = sensor_point(&stack, None, sensor, out);
                let back = output_point(&stack.geometry, sensor, s);
                assert!(
                    (back.0 - out.0).abs() < 1e-3 && (back.1 - out.1).abs() < 1e-3,
                    "turns {turns} mirror {mh}/{mv}: {back:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device of any kind — the no-op adapter is enough, since wgpu validates on the CPU.
    fn device() -> wgpu::Device {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::NOOP,
            backend_options: wgpu::BackendOptions {
                noop: wgpu::NoopBackendOptions::enabled(),
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        pollster::block_on(adapter.request_device(&Default::default()))
            .unwrap()
            .0
    }

    #[test]
    fn a_refused_allocation_is_an_error_not_a_panic() {
        let device = device();
        let too_big = device.limits().max_buffer_size + 1;
        let result = scoped(&device, "a huge buffer", || {
            device.make_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: too_big,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let err = result.expect_err("an over-limit buffer was accepted");
        assert!(err.to_string().starts_with("a huge buffer: "), "{err}");

        // The scopes are gone again: a good allocation afterwards is fine.
        scoped(&device, "a small buffer", || {
            device.make_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 256,
                usage: wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .unwrap();
    }

    #[test]
    fn scopes_dropped_early_pop_in_order() {
        // An early `?` drops the guards instead of popping them; wgpu panics if that
        // happens in the wrong order.
        let device = device();
        let outer = ErrorScopes::push(&device);
        drop(ErrorScopes::push(&device));
        drop(outer);
    }
}

// ── One-shot sensor reads ───────────────────────────────────────────────────────────────
//
// The white-balance eyedropper and the Auto buttons read the sensor back once, when asked:
// a user action, never a frame (DESIGN invariant 4 names scopes and export; these are the
// third sanctioned kind, and just as rare).

/// Where full-resolution output pixel `out_px` (in the turned, uncropped frame) came from
/// on the sensor: the shader's `sensor_coords`, on the CPU.
pub fn sensor_point(
    stack: &PhotoStack,
    warp: Option<&FileWarp>,
    sensor: (u32, u32),
    out_px: (f32, f32),
) -> (f32, f32) {
    let g = &stack.geometry;
    let (ow, oh) = g.output_size(sensor.0, sensor.1);
    let (mut x, mut y) = (out_px.0 - ow as f32 * 0.5, out_px.1 - oh as f32 * 0.5);
    if let Some(p) = g.perspective((ow, oh)) {
        (x, y) = p.source((x, y));
    }
    let (s, c) = (-g.straighten_deg.to_radians()).sin_cos();
    (x, y) = (x * c - y * s, x * s + y * c);
    if g.mirror_h {
        x = -x;
    }
    if g.mirror_v {
        y = -y;
    }
    for _ in 0..g.quarter_turns {
        (x, y) = (y, -x);
    }
    if stack.lens.enabled {
        (x, y) = LensModel::new(&stack.lens.params).distorted((x, y), sensor);
    }
    let ideal = (x + sensor.0 as f32 * 0.5, y + sensor.1 as f32 * 0.5);
    // Green's plane: what the eyedropper's patch is read around.
    warp.map_or(ideal, |w| w.source(1, ideal))
}

/// Where sensor pixel `sensor_px` lands in the turned, uncropped full-resolution frame:
/// `sensor_point`'s inverse, for the geometry alone (lens corrections move a point a few
/// pixels at most, which placing a mask handle does not need).
pub fn output_point(g: &Geometry, sensor: (u32, u32), sensor_px: (f32, f32)) -> (f32, f32) {
    let (ow, oh) = g.output_size(sensor.0, sensor.1);
    let (mut x, mut y) = (
        sensor_px.0 - sensor.0 as f32 * 0.5,
        sensor_px.1 - sensor.1 as f32 * 0.5,
    );
    for _ in 0..g.quarter_turns {
        (x, y) = (-y, x);
    }
    if g.mirror_h {
        x = -x;
    }
    if g.mirror_v {
        y = -y;
    }
    let (s, c) = g.straighten_deg.to_radians().sin_cos();
    (x, y) = (x * c - y * s, x * s + y * c);
    if let Some(p) = g.perspective((ow, oh)) {
        (x, y) = p.output((x, y));
    }
    (x + ow as f32 * 0.5, y + oh as f32 * 0.5)
}

/// What Auto tone chose: exposure and the four tonal controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoTone {
    pub ev: f32,
    pub tonal: crate::params::Tonal,
}

impl Engine {
    /// Sensor texels (rgb camera values before white balance, a = reconstructed neutral)
    /// of `rect` at mip `level`.
    fn sensor_texels(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        level: u32,
        rect: (u32, u32, u32, u32),
    ) -> Result<Vec<[f32; 4]>> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let bytes = read_back_level(
            device,
            queue,
            &image.sensor,
            level,
            rect,
            8,
            MAX_BAND_BYTES.min(device.limits().max_buffer_size),
        )?;
        Ok(bytes
            .chunks_exact(8)
            .map(|t| {
                std::array::from_fn(|i| half::f16::from_le_bytes([t[2 * i], t[2 * i + 1]]).to_f32())
            })
            .collect())
    }

    /// The whole sensor at the finest level no more than `side` texels across.
    fn sensor_overview(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        side: u32,
    ) -> Result<Vec<[f32; 4]>> {
        Ok(self.sensor_overview_sized(device, queue, side)?.0)
    }

    /// The sensor texture's finest level as it is stored (camera RGB, and the highlight
    /// neutral in alpha), for tests that check the demosaic itself.
    #[doc(hidden)]
    pub fn sensor_level0(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Overview> {
        self.sensor_overview_sized(device, queue, u32::MAX)
    }

    /// [`Self::sensor_overview`], and its size.
    fn sensor_overview_sized(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        side: u32,
    ) -> Result<Overview> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let mut level = 0;
        while level + 1 < image.mip_levels
            && image.sensor_width.max(image.sensor_height) >> level > side
        {
            level += 1;
        }
        // Mip sizes round down; rounding up would read a texel past an odd sensor's edge.
        let (w, h) = (
            (image.sensor_width >> level).max(1),
            (image.sensor_height >> level).max(1),
        );
        Ok((
            self.sensor_texels(device, queue, level, (0, 0, w, h))?,
            (w, h),
        ))
    }

    /// Where to take a heal spot's patch from, for a spot at `dst` of `radius` (sensor
    /// coordinates, long-side fraction): see `heal::find_source`. A one-shot read, as the
    /// eyedropper's is.
    pub fn heal_source(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: [f32; 2],
        radius: f32,
    ) -> Result<[f32; 2]> {
        // Fine enough that the spot is several texels across, and no finer.
        let side = (12.0 / radius.max(1e-3)).clamp(256.0, 1024.0) as u32;
        let (texels, size) = self.sensor_overview_sized(device, queue, side)?;
        Ok(crate::heal::find_source(&texels, size, dst, radius))
    }

    /// The white balance that makes the 5×5 patch around `point` (0..1 of the turned,
    /// uncropped frame, as `geometry` frames it) neutral. `None` over a clipped highlight or
    /// black, where the colour is not the scene's.
    pub fn neutral_at(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        point: (f32, f32),
    ) -> Result<Option<WhiteBalance>> {
        let geometry = &stack.geometry;
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let sensor = (image.sensor_width, image.sensor_height);
        let (ow, oh) = geometry.output_size(sensor.0, sensor.1);
        let warp = image.warp_for(stack);
        let (sx, sy) = sensor_point(
            stack,
            warp.as_ref(),
            sensor,
            (point.0 * ow as f32, point.1 * oh as f32),
        );
        if !(0.0..sensor.0 as f32).contains(&sx) || !(0.0..sensor.1 as f32).contains(&sy) {
            return Ok(None);
        }
        let x0 = (sx as u32)
            .saturating_sub(2)
            .min(sensor.0.saturating_sub(5));
        let y0 = (sy as u32)
            .saturating_sub(2)
            .min(sensor.1.saturating_sub(5));
        let patch =
            self.sensor_texels(device, queue, 0, (x0, y0, 5.min(sensor.0), 5.min(sensor.1)))?;
        Ok(neutralising(&patch, image.wb))
    }

    /// Sample the average RGB colour in a 5×5 patch around `point` (0..1 of the frame).
    pub fn sample_color_at(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        point: (f32, f32),
    ) -> Result<Option<[f32; 3]>> {
        let geometry = &stack.geometry;
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let sensor = (image.sensor_width, image.sensor_height);
        let (ow, oh) = geometry.output_size(sensor.0, sensor.1);
        let warp = image.warp_for(stack);
        let (sx, sy) = sensor_point(
            stack,
            warp.as_ref(),
            sensor,
            (point.0 * ow as f32, point.1 * oh as f32),
        );
        if !(0.0..sensor.0 as f32).contains(&sx) || !(0.0..sensor.1 as f32).contains(&sy) {
            return Ok(None);
        }
        let x0 = (sx as u32)
            .saturating_sub(2)
            .min(sensor.0.saturating_sub(5));
        let y0 = (sy as u32)
            .saturating_sub(2)
            .min(sensor.1.saturating_sub(5));
        let patch =
            self.sensor_texels(device, queue, 0, (x0, y0, 5.min(sensor.0), 5.min(sensor.1)))?;
        if patch.is_empty() {
            return Ok(None);
        }
        let mut r = 0.0f32;
        let mut g = 0.0f32;
        let mut b = 0.0f32;
        let wb = image.wb;
        for px in &patch {
            r += px[0] * wb[0];
            g += px[1] * wb[1];
            b += px[2] * wb[2];
        }
        let n = patch.len() as f32;
        Ok(Some([r / n, g / n, b / n]))
    }

    /// A white balance for the whole photo: grey-world and white-patch estimates on an
    /// overview of the sensor, clipped and near-black texels left out, and their geometric
    /// mean taken (each is wrong in its own way; together they rarely are).
    pub fn auto_white_balance(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Option<WhiteBalance>> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let texels = self.sensor_overview(device, queue, 128)?;
        let wb = image.wb;
        let bright = |t: &[f32; 4]| t[0] * wb[0] + t[1] * wb[1] + t[2] * wb[2];
        let peak = texels.iter().map(bright).fold(0.0f32, f32::max);
        let usable: Vec<[f32; 4]> = texels
            .into_iter()
            .filter(|t| {
                let b = bright(t);
                t.iter().all(|v| v.is_finite())
                    && t[3] <= 1e-4
                    && b > 0.03 * peak
                    && t[..3].iter().all(|&v| v > 0.0)
            })
            .collect();
        if usable.len() < 16 {
            return Ok(None);
        }
        let grey = neutralising(&usable, wb);
        let mut by_brightness = usable.clone();
        by_brightness.sort_by(|a, b| bright(b).total_cmp(&bright(a)));
        let top = &by_brightness[..(by_brightness.len() / 20).max(4)];
        let white = neutralising(top, wb);
        Ok(match (grey, white) {
            (Some(g), Some(w)) => Some(WhiteBalance {
                tint_r: (g.tint_r * w.tint_r).sqrt(),
                tint_b: (g.tint_b * w.tint_b).sqrt(),
            }),
            (one, other) => one.or(other),
        })
    }

    /// Exposure and tone for the photo as `stack` white-balances it: the median at mid
    /// grey, highlights pulled in when the brightest tones would clip, shadows lifted when
    /// the darkest would crush.
    /// Auto upright (W-07): the straighten and keystone that stand the photo's lines up,
    /// as far as `mode` corrects, found on an overview of the sensor. `None` when the
    /// photo has too few straight lines to go by.
    pub fn auto_upright(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        mode: crate::keystone::Upright,
    ) -> Result<Option<Geometry>> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let (texels, (w, h)) = self.sensor_overview_sized(device, queue, 512)?;
        let wb = image.wb;
        let lum: Vec<f32> = texels
            .iter()
            .map(|t| (t[0] * wb[0] + t[1] * wb[1] + t[2] * wb[2]) / 3.0 + t[3])
            .collect();
        let (vertical, horizontal) = crate::keystone::find_lines(&lum, w as usize, h as usize);
        // From the overview's pixels to the sensor's.
        let (sx, sy) = (
            image.sensor_width as f32 / w as f32,
            image.sensor_height as f32 / h as f32,
        );
        let scale = |segs: Vec<crate::keystone::Segment>| -> Vec<crate::keystone::Segment> {
            segs.into_iter()
                .map(|s| crate::keystone::Segment {
                    a: ((s.a.0 + 0.5) * sx, (s.a.1 + 0.5) * sy),
                    b: ((s.b.0 + 0.5) * sx, (s.b.1 + 0.5) * sy),
                })
                .collect()
        };
        Ok(crate::keystone::solve(
            &stack.geometry,
            (image.sensor_width, image.sensor_height),
            &scale(vertical),
            &scale(horizontal),
            mode,
        ))
    }

    pub fn auto_tone(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
    ) -> Result<Option<AutoTone>> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let texels = self.sensor_overview(device, queue, 128)?;
        let w = stack.white_balance.params;
        let wb = if stack.white_balance.enabled {
            [image.wb[0] * w.tint_r, image.wb[1], image.wb[2] * w.tint_b]
        } else {
            image.wb
        };
        let m = image.cam_to_srgb;
        let mut luma: Vec<f32> = texels
            .iter()
            .filter_map(|t| {
                let c = [0, 1, 2].map(|i| t[i] * wb[i] + t[3]);
                let rgb = [0, 1, 2].map(|r| m[r][0] * c[0] + m[r][1] * c[1] + m[r][2] * c[2]);
                let y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
                y.is_finite().then_some(y.max(0.0))
            })
            .collect();
        if luma.len() < 16 {
            return Ok(None);
        }
        luma.sort_by(f32::total_cmp);
        let at = |q: f32| luma[((luma.len() - 1) as f32 * q) as usize];
        let median = at(0.5).max(1e-5);
        let ev = (0.18 / median).log2().clamp(-4.0, 4.0);
        let gain = ev.exp2();
        let (high, low) = (at(0.998) * gain, at(0.005) * gain);
        let mut tonal = crate::params::Tonal::default();
        if high > 1.0 {
            // Stops over white, into the highlight control's range.
            tonal.highlights = -(high.log2() * 0.4).clamp(0.0, 0.8);
        }
        if low < 0.004 {
            tonal.shadows = ((0.004 / low.max(1e-6)).log2() * 0.08).clamp(0.0, 0.5);
        }
        Ok(Some(AutoTone { ev, tonal }))
    }
}

/// The white balance that turns the mean of `texels` grey, as tints on `as_shot`.
fn neutralising(texels: &[[f32; 4]], as_shot: [f32; 3]) -> Option<WhiteBalance> {
    let n = texels.len() as f32;
    if n == 0.0 {
        return None;
    }
    let mean = |i: usize| texels.iter().map(|t| t[i]).sum::<f32>() / n;
    let (r, g, b, clipped) = (mean(0), mean(1), mean(2), mean(3));
    // A patch mostly rebuilt from its neutral (a clipped highlight) says nothing about the
    // light's colour.
    if !(r > 0.0 && g > 0.0 && b > 0.0) || clipped > 0.25 * g * as_shot[1] {
        return None;
    }
    let tint_r = (g * as_shot[1]) / (r * as_shot[0]);
    let tint_b = (g * as_shot[1]) / (b * as_shot[2]);
    (tint_r.is_finite() && tint_b.is_finite()).then(|| WhiteBalance {
        tint_r: tint_r.clamp(0.2, 5.0),
        tint_b: tint_b.clamp(0.2, 5.0),
    })
}

#[cfg(test)]
mod uniform_layout_tests {
    use super::*;

    /// The Rust uniform struct and the shader's must agree member by member: a field added
    /// in a different place on each side reads another's bytes, and nothing else notices.
    #[test]
    fn the_adjust_uniforms_match_the_shader_member_by_member() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("shaders/adjust.wgsl"))
            .expect("adjust.wgsl parses");
        let mut layouter = wgpu::naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).unwrap();
        let (handle, ty) = module
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("Adjust"))
            .expect("struct Adjust");
        let wgpu::naga::TypeInner::Struct { members, .. } = &ty.inner else {
            panic!("Adjust is not a struct")
        };
        let rust = [
            ("cam0", std::mem::offset_of!(AdjustUniforms, cam0)),
            ("cam1", std::mem::offset_of!(AdjustUniforms, cam1)),
            ("cam2", std::mem::offset_of!(AdjustUniforms, cam2)),
            ("wb", std::mem::offset_of!(AdjustUniforms, wb)),
            ("exposure", std::mem::offset_of!(AdjustUniforms, exposure)),
            ("filmic", std::mem::offset_of!(AdjustUniforms, filmic)),
            ("lift", std::mem::offset_of!(AdjustUniforms, lift)),
            ("gamma", std::mem::offset_of!(AdjustUniforms, gamma)),
            ("gain", std::mem::offset_of!(AdjustUniforms, gain)),
            ("offset", std::mem::offset_of!(AdjustUniforms, offset)),
            ("flags", std::mem::offset_of!(AdjustUniforms, flags)),
            ("sizes", std::mem::offset_of!(AdjustUniforms, sizes)),
            ("orient", std::mem::offset_of!(AdjustUniforms, orient)),
            ("turns", std::mem::offset_of!(AdjustUniforms, turns)),
            ("split", std::mem::offset_of!(AdjustUniforms, split)),
            (
                "as_shot_wb",
                std::mem::offset_of!(AdjustUniforms, as_shot_wb),
            ),
            ("tonal", std::mem::offset_of!(AdjustUniforms, tonal)),
            ("vibrance", std::mem::offset_of!(AdjustUniforms, vibrance)),
            ("vignette", std::mem::offset_of!(AdjustUniforms, vignette)),
            ("sharpen", std::mem::offset_of!(AdjustUniforms, sharpen)),
            ("dest", std::mem::offset_of!(AdjustUniforms, dest)),
            ("detail", std::mem::offset_of!(AdjustUniforms, detail)),
            ("eq_red", std::mem::offset_of!(AdjustUniforms, eq_red)),
            ("eq_orange", std::mem::offset_of!(AdjustUniforms, eq_orange)),
            ("eq_yellow", std::mem::offset_of!(AdjustUniforms, eq_yellow)),
            ("eq_green", std::mem::offset_of!(AdjustUniforms, eq_green)),
            ("eq_aqua", std::mem::offset_of!(AdjustUniforms, eq_aqua)),
            ("eq_blue", std::mem::offset_of!(AdjustUniforms, eq_blue)),
            ("eq_purple", std::mem::offset_of!(AdjustUniforms, eq_purple)),
            (
                "eq_magenta",
                std::mem::offset_of!(AdjustUniforms, eq_magenta),
            ),
            ("frame", std::mem::offset_of!(AdjustUniforms, frame)),
            ("lens", std::mem::offset_of!(AdjustUniforms, lens)),
            ("lens_b", std::mem::offset_of!(AdjustUniforms, lens_b)),
            ("warp", std::mem::offset_of!(AdjustUniforms, warp)),
            ("warp_c", std::mem::offset_of!(AdjustUniforms, warp_c)),
            ("tca_r", std::mem::offset_of!(AdjustUniforms, tca_r)),
            ("tca_b", std::mem::offset_of!(AdjustUniforms, tca_b)),
            ("warp_vig", std::mem::offset_of!(AdjustUniforms, warp_vig)),
            ("masks", std::mem::offset_of!(AdjustUniforms, masks)),
            ("mask_info", std::mem::offset_of!(AdjustUniforms, mask_info)),
            ("spots", std::mem::offset_of!(AdjustUniforms, spots)),
            ("persp", std::mem::offset_of!(AdjustUniforms, persp)),
            ("luma_w", std::mem::offset_of!(AdjustUniforms, luma_w)),
            ("out_m", std::mem::offset_of!(AdjustUniforms, out_m)),
            ("proof_m", std::mem::offset_of!(AdjustUniforms, proof_m)),
            ("hdr", std::mem::offset_of!(AdjustUniforms, hdr)),
        ];
        assert_eq!(members.len(), rust.len(), "a member on one side only");
        for (m, (name, offset)) in members.iter().zip(rust) {
            assert_eq!(m.name.as_deref(), Some(name), "members out of order");
            assert_eq!(m.offset as usize, offset, "{name} is at a different offset");
        }
        assert_eq!(
            layouter[handle].size as usize,
            std::mem::size_of::<AdjustUniforms>(),
            "sizes differ"
        );
    }
}
