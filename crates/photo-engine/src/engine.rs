//! The GPU side: upload once, demosaic once, adjust per frame.
//!
//! `Engine` never creates a `Device` and never touches a window. It is handed both by
//! whatever shell is running (eframe on desktop, `android-activity` later), which is what
//! makes an Android build a new bootstrap rather than a second implementation.

use anyhow::{bail, Result};
use wgpu::util::DeviceExt;

use crate::curve::{build_lut, CurveNode, LUT_SIZE};
use crate::decode::CfaImage;
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

/// Linear camera RGB, half float. 16 bits is the point: 8 would quantise the headroom
/// that makes highlight recovery possible at all.
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
const HISTOGRAM_STRIDE: u32 = 4;

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
}

pub struct LoadedImage {
    /// Output dimensions, after right-angle turns. What display and export work in.
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
    /// What egui draws. Also the export source.
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
}

pub struct Engine {
    green_pipeline: wgpu::RenderPipeline,
    green_bgl: wgpu::BindGroupLayout,
    rb_pipeline: wgpu::RenderPipeline,
    rb_bgl: wgpu::BindGroupLayout,
    adjust_pipeline: wgpu::RenderPipeline,
    adjust_bgl: wgpu::BindGroupLayout,
    adjust_buffer: wgpu::Buffer,
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
            lut_texture,
            lut_view,
            sensor_sampler,
            cached_curve: Vec::new(),
            image: None,
        }
    }

    pub fn image(&self) -> Option<&LoadedImage> {
        self.image.as_ref()
    }

    pub fn unload(&mut self) {
        self.image = None;
        self.hist_epoch = self.hist_epoch.wrapping_add(1);
        self.histogram = Histogram::default();
    }

    /// Upload, demosaic, and keep only the linear RGB result.
    ///
    /// The mosaic texture is dropped at the end of this function: it has served its
    /// purpose and the sensor texture is immutable from here (invariant 5).
    pub fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cfa: CfaImage,
    ) -> Result<()> {
        let limit = device.limits().max_texture_dimension_2d;
        if cfa.width > limit || cfa.height > limit {
            bail!(
                "{}×{} exceeds this GPU's {}px texture limit",
                cfa.width,
                cfa.height,
                limit
            );
        }

        let extent = wgpu::Extent3d {
            width: cfa.width,
            height: cfa.height,
            depth_or_array_layers: 1,
        };

        let mosaic = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cfa mosaic"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &mosaic,
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

        let sensor = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sensor rgb"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SENSOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let sensor_view = sensor.create_view(&Default::default());

        let green = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("green plane"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GREEN_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let green_view = green.create_view(&Default::default());

        let demosaic_uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("demosaic uniforms"),
            contents: bytemuck::bytes_of(&DemosaicUniforms {
                size: [cfa.width, cfa.height, 0, 0],
                cfa: cfa.cfa2x2,
                black: cfa.black,
                white: cfa.white,
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

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("demosaic encoder"),
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
            &sensor_view,
        );
        queue.submit([encoder.finish()]);

        let geometry = Geometry::default();
        let (out_w, out_h) = geometry.output_size(cfa.width, cfa.height);
        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("adjusted output"),
            size: wgpu::Extent3d {
                width: out_w,
                height: out_h,
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
        });
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
            sensor_width: cfa.width,
            sensor_height: cfa.height,
            geometry,
            make: cfa.make,
            model: cfa.model,
            wb: cfa.wb,
            cam_to_srgb: cfa.cam_to_srgb,
            output_view,
            output_texture,
            adjust_bind,
        });
        // The mosaic, the green plane and their bind groups drop here. Only linear RGB
        // survives, and it is immutable from this point (invariant 5).
        Ok(())
    }

    /// Read the adjusted image back off the GPU.
    ///
    /// This copies the very texture the screen is showing — invariant 2 is satisfied by
    /// there being nothing else to copy. The caller is expected to have rendered the
    /// current stack first; `render` is cheap and idempotent, so callers just call it.
    ///
    /// Blocks until the GPU has finished and the buffer is mapped. At 24 MP the readback
    /// is ~98 MB, so this belongs off the UI thread in anything interactive.
    pub fn export(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        crop: Crop,
    ) -> Result<ExportedImage> {
        let Some(image) = &self.image else {
            bail!("no image is loaded");
        };
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

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("export encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &image.output_texture,
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

        image.output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("adjusted output"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
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
        });
        image.output_view = image.output_texture.create_view(&Default::default());
        image.width = w;
        image.height = h;
        // Whatever readback is in flight describes a texture that no longer exists.
        self.hist_epoch = self.hist_epoch.wrapping_add(1);
        self.histogram = Histogram::default();
        true
    }

    /// Re-run the adjustment chain. This is the per-frame path and the per-slider-tick
    /// path — it writes one uniform buffer and submits one fullscreen draw.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stack: &PhotoStack) {
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
                    0.0,
                    0.0,
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
            }),
        );

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
            let (ox, oy, w, h) = g.crop.to_pixels(image.width, image.height);
            queue.write_buffer(
                &self.hist_params,
                0,
                bytemuck::bytes_of(&HistogramParams {
                    region: [ox, oy, w, h],
                    stride: [HISTOGRAM_STRIDE, 0, 0, 0],
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
                let groups_x = w.div_ceil(HISTOGRAM_STRIDE).div_ceil(16).max(1);
                let groups_y = h.div_ceil(HISTOGRAM_STRIDE).div_ceil(16).max(1);
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
