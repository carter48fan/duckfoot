//! The GPU side: upload once, demosaic once, adjust per frame.
//!
//! `Engine` never creates a `Device` and never touches a window. It is handed both by
//! whatever shell is running (eframe on desktop, `android-activity` later), which is what
//! makes an Android build a new bootstrap rather than a second implementation.

use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use wgpu::util::DeviceExt;

use crate::curve::{build_lut, CurveNode, LUT_SIZE};
use crate::decode::{CfaImage, RasterImage};
use crate::export::{padded_bytes_per_row, unpad_rows, ExportedImage};
use crate::histogram::{Histogram, ReadbackState, BUFFER_BYTES};
use crate::params::{Crop, Geometry, PhotoStack};

/// The adjusted image is written here, and egui samples it directly.
///
/// Deliberately NOT an `Srgb` format. egui asks for a non-sRGB surface and writes
/// gamma-encoded colour into it; if this texture were sRGB the hardware would decode on
/// sample and re-encode on present, and the image would land a gamma apart from the UI
/// around it. So there is exactly one transfer function in the whole pipeline and it is
/// written by hand at the end of adjust.wgsl.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Linear camera RGB, half float, with the reconstructed-highlight neutral in alpha
/// (demosaic_rb.wgsl). 16 bits is the point: 8 would quantise the headroom that makes
/// highlight recovery possible at all.
const SENSOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The intermediate green plane. Half float is ample — it only has to carry one channel
/// of a signal that is about to be added back to a colour difference.
const GREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

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
    size: [u32; 4],
    cfa: [u32; 4],
    black: [f32; 4],
    white: [f32; 4],
}

/// Everything the adjustment chain needs, in one buffer. A slider drag rewrites exactly
/// this — 176 bytes — and nothing else (DESIGN.md invariant 6).
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
    /// Levels in the sensor texture's mip chain; level 0 is full resolution.
    mip_levels: u32,
    /// Kept so its memory can be recycled when this image is replaced by one the same size.
    sensor: wgpu::Texture,
    sensor_extent: wgpu::Extent3d,
    /// The preview renders at 1/2^level of full resolution. 0 = full.
    preview_level: u32,
    /// The preview: what egui draws. Sized by `preview_level`, not by the sensor.
    pub output_view: wgpu::TextureView,
    output_texture: wgpu::Texture,
    adjust_bind: wgpu::BindGroup,
}

impl LoadedImage {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
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
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("adjusted output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

pub struct Engine {
    green_pipeline: wgpu::RenderPipeline,
    green_bgl: wgpu::BindGroupLayout,
    rb_pipeline: wgpu::RenderPipeline,
    rb_bgl: wgpu::BindGroupLayout,
    adjust_pipeline: wgpu::RenderPipeline,
    adjust_bgl: wgpu::BindGroupLayout,
    adjust_buffer: wgpu::Buffer,
    mip_pipeline: wgpu::RenderPipeline,
    mip_bgl: wgpu::BindGroupLayout,
    lut_texture: wgpu::Texture,
    lut_view: wgpu::TextureView,
    sensor_sampler: wgpu::Sampler,
    cached_curve: Vec<CurveNode>,
    image: Option<LoadedImage>,

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
    pub make: String,
    pub model: String,
}

enum UploadedPixels {
    /// The sensor mosaic, still to be demosaiced.
    Mosaic {
        texture: wgpu::Texture,
        cfa: [u32; 4],
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
    let texture = pool.take(extent).unwrap_or_else(|| {
        device.create_texture(&wgpu::TextureDescriptor {
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
    Ok(UploadedImage {
        pixels: UploadedPixels::Mosaic {
            texture,
            cfa: cfa.cfa2x2,
            black: cfa.black,
            white: cfa.white,
        },
        extent,
        geometry: Geometry::default(),
        orientation: cfa.orientation,
        wb: cfa.wb,
        cam_to_srgb: cfa.cam_to_srgb,
        make: cfa.make,
        model: cfa.model,
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
    let texture = device.create_texture(&wgpu::TextureDescriptor {
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
    Ok(UploadedImage {
        pixels: UploadedPixels::Linear { texture },
        extent,
        geometry: raster.orientation,
        orientation: raster.orientation,
        wb: [1.0, 1.0, 1.0],
        cam_to_srgb: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        make: raster.make,
        model: raster.model,
    })
}

impl Engine {
    pub fn new(device: &wgpu::Device) -> Self {
        let green_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("demosaic green"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/demosaic_green.wgsl").into()),
        });
        let rb_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("demosaic rb"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/demosaic_rb.wgsl").into()),
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
        let mip_pipeline = fullscreen_pipeline(
            device,
            "sensor downsample",
            &mip_shader,
            &mip_bgl,
            SENSOR_FORMAT,
        );

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
            ],
        });

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

        let green_pipeline = fullscreen_pipeline(
            device,
            "demosaic green",
            &green_shader,
            &green_bgl,
            GREEN_FORMAT,
        );
        let rb_pipeline =
            fullscreen_pipeline(device, "demosaic rb", &rb_shader, &rb_bgl, SENSOR_FORMAT);
        let adjust_pipeline =
            fullscreen_pipeline(device, "adjust", &adjust_shader, &adjust_bgl, OUTPUT_FORMAT);

        let adjust_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adjust uniforms"),
            size: std::mem::size_of::<AdjustUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let lut_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tone curve lut"),
            size: wgpu::Extent3d {
                width: LUT_SIZE as u32,
                height: 1,
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
        let hist_bins = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("histogram bins"),
            size: BUFFER_BYTES,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hist_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("histogram staging"),
            size: BUFFER_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let hist_params = device.create_buffer(&wgpu::BufferDescriptor {
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
            green_pipeline,
            green_bgl,
            rb_pipeline,
            rb_bgl,
            adjust_pipeline,
            adjust_bgl,
            adjust_buffer,
            mip_pipeline,
            mip_bgl,
            lut_texture,
            lut_view,
            sensor_sampler,
            cached_curve: Vec::new(),
            image: None,
            preview_level: 0,
            scratch: None,
            retired_sensor: None,
            upload_pool: UploadPool::default(),
        }
    }

    pub fn image(&self) -> Option<&LoadedImage> {
        self.image.as_ref()
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
            make,
            model,
        } = uploaded;
        let (width, height) = (extent.width, extent.height);

        let mip_levels = mip_count(width, height);
        let sensor = match self.retired_sensor.take() {
            Some((e, m, texture)) if e == extent && m == mip_levels => texture,
            _ => device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sensor rgb"),
                size: extent,
                mip_level_count: mip_levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SENSOR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST,
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
                        green: device.create_texture(&wgpu::TextureDescriptor {
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
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("demosaic uniforms"),
                        contents: bytemuck::bytes_of(&DemosaicUniforms {
                            size: [width, height, 0, 0],
                            cfa,
                            black,
                            white,
                        }),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

                let mosaic_view = mosaic.create_view(&Default::default());
                let green_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                let rb_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
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

                // Green first and alone: red and blue are reconstructed relative to it.
                draw_fullscreen(
                    &mut encoder,
                    "demosaic green pass",
                    &self.green_pipeline,
                    &green_bind,
                    &green_view,
                );
                draw_fullscreen(
                    &mut encoder,
                    "demosaic rb pass",
                    &self.rb_pipeline,
                    &rb_bind,
                    &level_view(0),
                );
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
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
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

        let adjust_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adjust bind"),
            layout: &self.adjust_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&sensor_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.adjust_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sensor_sampler),
                },
            ],
        });

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
            mip_levels,
            preview_level,
            sensor_extent: extent,
            sensor,
            output_view,
            output_texture,
            adjust_bind,
        });
        Ok(())
    }

    /// Render the stack at full resolution and read it back off the GPU.
    ///
    /// Same shader, same uniforms as the preview; only the render target and the sensor
    /// level differ (invariant 2, and DESIGN.md "Preview and export"). The full-resolution
    /// target exists only for the length of this call, so a preview-sized session never
    /// holds a second copy of a 24 MP frame.
    ///
    /// Blocks until the GPU has finished and the buffer is mapped. At 24 MP the readback
    /// is ~98 MB, so this belongs off the UI thread in anything interactive. The preview's
    /// uniforms are overwritten, so render again before the next frame is shown.
    pub fn export(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        crop: Crop,
    ) -> Result<ExportedImage> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let full = output_texture(device, image.width, image.height);
        let full_view = full.create_view(&Default::default());
        self.write_uniforms(queue, stack, None, (image.width, image.height), 0);

        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("export encoder"),
        });
        draw_fullscreen(
            &mut encoder,
            "export adjust pass",
            &self.adjust_pipeline,
            &image.adjust_bind,
            &full_view,
        );

        // The crop is not baked into the texture, so export is where it becomes real: copy
        // only the region the user framed. See `params::Crop` for why it is not in the shader.
        let (ox, oy, width, height) = crop.to_pixels(image.width, image.height);

        let stride = padded_bytes_per_row(width);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("export readback"),
            size: stride as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &full,
                mip_level: 0,
                origin: wgpu::Origin3d { x: ox, y: oy, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;

        let rgba = {
            let mapped = slice.get_mapped_range()?;
            unpad_rows(&mapped, width, height)?
        };
        buffer.unmap();

        Ok(ExportedImage {
            width,
            height,
            rgba,
        })
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
        image.output_texture = output_texture(device, w, h);
        image.output_view = image.output_texture.create_view(&Default::default());
        // Whatever readback is in flight describes a texture that no longer exists.
        self.hist_epoch = self.hist_epoch.wrapping_add(1);
        self.histogram = Histogram::default();
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
        let Some(image) = &self.image else { return };
        let target = image.preview_size();
        let level = image.preview_level;
        self.write_uniforms(queue, stack, split_x, target, level);
        let Some(image) = &self.image else { return };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("adjust encoder"),
        });
        draw_fullscreen(
            &mut encoder,
            "adjust pass",
            &self.adjust_pipeline,
            &image.adjust_bind,
            &image.output_view,
        );

        // Only queue a new histogram when the previous one has been collected. Otherwise a
        // fast drag would pile up map requests against a buffer that is already mapped.
        let measuring = self.hist_state.is_idle();
        if measuring {
            // Scopes describe what the photograph will be, so they measure inside the crop.
            // A histogram of pixels the user has framed out is actively misleading.
            let (ox, oy, w, h) = stack.geometry.crop.to_pixels(target.0, target.1);
            // A preview at level L already has 4^L fewer pixels; keep roughly the same
            // sample count rather than striding over what is left.
            let stride = (HISTOGRAM_STRIDE >> level).max(1);
            queue.write_buffer(
                &self.hist_params,
                0,
                bytemuck::bytes_of(&HistogramParams {
                    region: [ox, oy, w, h],
                    stride: [stride, 0, 0, 0],
                }),
            );
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            });

            encoder.clear_buffer(&self.hist_bins, 0, None);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("histogram pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.hist_pipeline);
                pass.set_bind_group(0, &bind, &[]);
                let groups_x = w.div_ceil(stride).div_ceil(16).max(1);
                let groups_y = h.div_ceil(stride).div_ceil(16).max(1);
                debug_assert!(groups_x > 0 && groups_y > 0);
                pass.dispatch_workgroups(groups_x, groups_y, 1);
            }
            encoder.copy_buffer_to_buffer(&self.hist_bins, 0, &self.hist_staging, 0, BUFFER_BYTES);
        }

        queue.submit([encoder.finish()]);

        if measuring {
            self.hist_inflight_epoch = self.hist_epoch;
            self.hist_state.set_pending();
            let state = self.hist_state.clone();
            self.hist_staging
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    // A failed map leaves the previous histogram on screen, which is a
                    // better outcome than a panic in a scope widget.
                    if result.is_ok() {
                        state.set_ready();
                    } else {
                        state.set_idle();
                    }
                });
        }
    }

    /// Everything a pass needs, into the one uniform buffer, plus the curve LUT if it
    /// changed. `target` is the render target's size and `level` the sensor mip to sample.
    fn write_uniforms(
        &mut self,
        queue: &wgpu::Queue,
        stack: &PhotoStack,
        split_x: Option<f32>,
        target: (u32, u32),
        level: u32,
    ) {
        let Some(image) = &self.image else { return };

        if stack.tone_curve.enabled && self.cached_curve != stack.tone_curve.params {
            let lut = build_lut(&stack.tone_curve.params);
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
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: LUT_SIZE as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.cached_curve = stack.tone_curve.params.clone();
        }

        let g = stack.geometry;
        let wb = stack.white_balance.params;
        let m = image.cam_to_srgb;
        let f = stack.filmic.params;
        let cb = stack.color_balance.params;

        queue.write_buffer(
            &self.adjust_buffer,
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
                flags: [stack.bitmask(), 0, 0, 0],
                sizes: [
                    image.width as f32,
                    image.height as f32,
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
                dest: [target.0 as f32, target.1 as f32, level as f32, 0.0],
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
            }),
        );
    }

    /// The most recent histogram. Never blocks: if the GPU has not finished, the previous
    /// one is returned and the plot is one interaction stale, which is invisible.
    pub fn histogram(&self) -> &Histogram {
        &self.histogram
    }

    /// Collect a finished histogram readback. Cheap, and safe to call every frame.
    pub fn poll_histogram(&mut self, device: &wgpu::Device) {
        // Drives wgpu's map callbacks; without this the readback never completes.
        let _ = device.poll(wgpu::PollType::Poll);
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

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
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
            entry_point: Some("fs"),
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
