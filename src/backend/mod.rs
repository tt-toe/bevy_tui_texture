//! WGPU-based ratatui backend implementation.
//!
//! This module provides a custom ratatui backend that renders terminal UIs to GPU textures
//! using WGPU. It's designed specifically for Bevy integration with zero-copy rendering
//! and efficient glyph caching.
//!
//! ## Architecture
//!
//! The backend consists of several key components:
//!
//! - **[`bevy_backend::BevyTerminalBackend`]** - Main backend implementation for ratatui
//! - **Text Atlas** - GPU texture cache for rendered glyphs (1800x1200px)
//! - **Render Pipelines** - Separate pipelines for background and foreground rendering
//! - **Programmatic Glyphs** - Special handling for box-drawing, braille, and block elements
//!
//! ## Rendering Pipeline
//!
//! The backend uses a two-pass rendering approach:
//!
//! 1. **Background Pass** - Renders cell backgrounds as colored quads
//! 2. **Foreground Pass** - Renders text glyphs from the atlas with alpha blending
//!
//! This separation allows for efficient rendering of terminal backgrounds and ensures
//! proper layering of text over backgrounds.
//!
//! ## Glyph Caching
//!
//! Glyphs are rasterized on-demand and cached in a GPU texture atlas:
//!
//! - First use of a glyph triggers rasterization using `rustybuzz` and `raqote`
//! - Cached glyphs are reused across frames with minimal overhead
//! - Atlas eviction policy ensures efficient memory usage
//! - Special programmatic glyphs (box-drawing, braille) are pre-populated
//!
//! ## Performance Optimizations
//!
//! - **Dirty Tracking** - Only re-render changed terminal cells
//! - **Batch Rendering** - Minimize draw calls by batching similar operations
//! - **Smart Cache Updates** - Defer GPU uploads until render time
//! - **Unicode Shaping** - Full Unicode support with complex text layout

pub mod bevy_backend;
pub(crate) mod programmatic_glyphs;
pub(crate) mod rasterize;

/// Width of the glyph cache texture in pixels.
///
/// The cache is large enough to store hundreds of unique glyphs simultaneously.
/// Dimensions are chosen to balance memory usage with cache hit rate.
pub(crate) const CACHE_WIDTH: u32 = 1800;

/// Height of the glyph cache texture in pixels.
pub(crate) const CACHE_HEIGHT: u32 = 1200;

// Compositor builders
use wgpu::*;

pub(crate) fn build_text_bg_compositor(
    device: &Device,
    screen_size: &Buffer,
    format: TextureFormat,
) -> TextCacheBgPipeline {
    let bg_shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("BG Compositor Shader"),
        source: ShaderSource::Wgsl(include_str!("shaders/composite_bg.wgsl").into()),
    });

    let bg_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("BG Bind Group Layout"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let bg_bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("BG Bind Group"),
        layout: &bg_bind_group_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: screen_size.as_entire_binding(),
        }],
    });

    let bg_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("BG Pipeline Layout"),
        bind_group_layouts: &[&bg_bind_group_layout],
        push_constant_ranges: &[],
    });

    let bg_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("BG Pipeline"),
        layout: Some(&bg_pipeline_layout),
        vertex: VertexState {
            module: &bg_shader,
            entry_point: Some("vs_main"),
            buffers: &[VertexBufferLayout {
                array_stride: std::mem::size_of::<TextBgVertexMember>() as BufferAddress,
                step_mode: VertexStepMode::Vertex,
                attributes: &vertex_attr_array![0 => Float32x2, 1 => Uint32],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(FragmentState {
            module: &bg_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(BlendState::REPLACE),
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    TextCacheBgPipeline {
        pipeline: bg_pipeline,
        fs_uniforms: bg_bind_group,
    }
}

pub(crate) fn build_text_fg_compositor(
    device: &Device,
    screen_size: &Buffer,
    atlas_size: &Buffer,
    cache: &TextureView,
    mask: &TextureView,
    sampler: &Sampler,
    format: TextureFormat,
) -> TextCacheFgPipeline {
    let fg_shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("FG Compositor Shader"),
        source: ShaderSource::Wgsl(include_str!("shaders/composite_fg.wgsl").into()),
    });

    let fg_bind_group_layout_0 = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("FG Bind Group Layout 0"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let fg_bind_group_0 = device.create_bind_group(&BindGroupDescriptor {
        label: Some("FG Bind Group 0"),
        layout: &fg_bind_group_layout_0,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: screen_size.as_entire_binding(),
        }],
    });

    let fg_bind_group_layout_1 = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("FG Bind Group Layout 1"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let fg_bind_group_1 = device.create_bind_group(&BindGroupDescriptor {
        label: Some("FG Bind Group 1"),
        layout: &fg_bind_group_layout_1,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(cache),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(mask),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(sampler),
            },
            BindGroupEntry {
                binding: 3,
                resource: atlas_size.as_entire_binding(),
            },
        ],
    });

    let fg_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("FG Pipeline Layout"),
        bind_group_layouts: &[&fg_bind_group_layout_0, &fg_bind_group_layout_1],
        push_constant_ranges: &[],
    });

    let fg_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("FG Pipeline"),
        layout: Some(&fg_pipeline_layout),
        vertex: VertexState {
            module: &fg_shader,
            entry_point: Some("vs_main"),
            buffers: &[VertexBufferLayout {
                array_stride: std::mem::size_of::<TextVertexMember>() as BufferAddress,
                step_mode: VertexStepMode::Vertex,
                attributes: &vertex_attr_array![
                    0 => Float32x2,
                    1 => Float32x2,
                    2 => Uint32,
                    3 => Uint32,
                    4 => Uint32
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(FragmentState {
            module: &fg_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    TextCacheFgPipeline {
        pipeline: fg_pipeline,
        fs_uniforms: fg_bind_group_0,
        atlas_bindings: fg_bind_group_1,
    }
}

use std::num::NonZeroU32;

use log::error;
use ratatui::style::Color;
use wgpu::Adapter;
use wgpu::BindGroup;
#[cfg(test)]
use wgpu::Buffer;
#[cfg(test)]
use wgpu::BufferDescriptor;
#[cfg(test)]
use wgpu::BufferUsages;
use wgpu::Device;
use wgpu::Extent3d;
use wgpu::RenderPipeline;
use wgpu::Surface;
use wgpu::SurfaceConfiguration;
use wgpu::SurfaceTexture;
#[cfg(test)]
use wgpu::Texture;
use wgpu::TextureDescriptor;
use wgpu::TextureDimension;
use wgpu::TextureFormat;
use wgpu::TextureUsages;
use wgpu::TextureView;
use wgpu::TextureViewDescriptor;

use crate::colors::ANSI_TO_RGB;
use crate::colors::Rgb;
use crate::colors::named::*;

/// The surface dimensions of the backend in pixels.
pub struct Dimensions {
    pub width: NonZeroU32,
    pub height: NonZeroU32,
}

impl From<(NonZeroU32, NonZeroU32)> for Dimensions {
    fn from((width, height): (NonZeroU32, NonZeroU32)) -> Self {
        Self { width, height }
    }
}

/// Controls the area the text is rendered to relative to the presentation
/// surface.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub enum Viewport {
    /// Render to the entire surface.
    #[default]
    Full,
    /// Render to a reduced area starting at the top right and rendering up to
    /// the bottom left - (width, height).
    Shrink { width: u32, height: u32 },
}

mod private {
    use wgpu::Surface;

    #[cfg(test)]
    use super::HeadlessSurface;
    #[cfg(test)]
    use super::HeadlessTarget;
    use super::RenderTarget;

    pub trait Sealed {}

    pub struct Token;

    impl Sealed for Surface<'_> {}
    impl Sealed for RenderTarget {}

    #[cfg(test)]
    impl Sealed for HeadlessTarget {}

    #[cfg(test)]
    impl Sealed for HeadlessSurface {}
}

/// A Texture target that can be rendered to.
pub trait RenderTexture: private::Sealed + Sized {
    /// Gets a [`wgpu::TextureView`] that can be used for rendering.
    fn get_view(&self, _token: private::Token) -> &TextureView;
    /// Presents the rendered result if applicable.
    fn present(self, _token: private::Token) {}
}

impl RenderTexture for RenderTarget {
    fn get_view(&self, _token: private::Token) -> &TextureView {
        &self.view
    }

    fn present(self, _token: private::Token) {
        self.texture.present();
    }
}

#[cfg(test)]
impl RenderTexture for HeadlessTarget {
    fn get_view(&self, _token: private::Token) -> &TextureView {
        &self.view
    }
}

/// A surface that can be rendered to.
pub trait RenderSurface<'s>: private::Sealed {
    type Target: RenderTexture;

    fn wgpu_surface(&self, _token: private::Token) -> Option<&Surface<'s>>;

    fn get_default_config(
        &self,
        adapter: &Adapter,
        width: u32,
        height: u32,
        _token: private::Token,
    ) -> Option<SurfaceConfiguration>;

    fn configure(&mut self, device: &Device, config: &SurfaceConfiguration, _token: private::Token);

    fn get_current_texture(&self, _token: private::Token) -> Option<Self::Target>;
}

pub struct RenderTarget {
    texture: SurfaceTexture,
    view: TextureView,
}

impl<'s> RenderSurface<'s> for Surface<'s> {
    type Target = RenderTarget;

    fn wgpu_surface(&self, _token: private::Token) -> Option<&Surface<'s>> {
        Some(self)
    }

    fn get_default_config(
        &self,
        adapter: &Adapter,
        width: u32,
        height: u32,
        _token: private::Token,
    ) -> Option<SurfaceConfiguration> {
        self.get_default_config(adapter, width, height)
    }

    fn configure(
        &mut self,
        device: &Device,
        config: &SurfaceConfiguration,
        _token: private::Token,
    ) {
        Surface::configure(self, device, config);
    }

    fn get_current_texture(&self, _token: private::Token) -> Option<Self::Target> {
        let output = match self.get_current_texture() {
            Ok(output) => output,
            Err(err) => {
                error!("{err}");
                return None;
            }
        };

        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        Some(RenderTarget {
            texture: output,
            view,
        })
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) struct HeadlessTarget {
    view: TextureView,
}

#[cfg(test)]
pub(crate) struct HeadlessSurface {
    pub(crate) texture: Option<Texture>,
    pub(crate) buffer: Option<Buffer>,
    pub(crate) buffer_width: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: TextureFormat,
}

#[cfg(test)]
impl HeadlessSurface {
    #[allow(dead_code)]
    fn new(format: TextureFormat) -> Self {
        Self {
            format,
            ..Default::default()
        }
    }
}

#[cfg(test)]
impl Default for HeadlessSurface {
    fn default() -> Self {
        Self {
            texture: Default::default(),
            buffer: Default::default(),
            buffer_width: Default::default(),
            width: Default::default(),
            height: Default::default(),
            format: TextureFormat::Rgba8Unorm,
        }
    }
}

#[cfg(test)]
impl RenderSurface<'static> for HeadlessSurface {
    type Target = HeadlessTarget;

    fn wgpu_surface(&self, _token: private::Token) -> Option<&Surface<'static>> {
        None
    }

    fn get_default_config(
        &self,
        _adapter: &Adapter,
        width: u32,
        height: u32,
        _token: private::Token,
    ) -> Option<SurfaceConfiguration> {
        Some(SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: self.format,
            width,
            height,
            present_mode: wgpu::PresentMode::Immediate,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        })
    }

    fn configure(
        &mut self,
        device: &Device,
        config: &SurfaceConfiguration,
        _token: private::Token,
    ) {
        self.texture = Some(device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: self.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        }));

        self.buffer_width = config.width * 4;
        self.buffer = Some(device.create_buffer(&BufferDescriptor {
            label: None,
            size: (self.buffer_width * config.height) as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
        self.width = config.width;
        self.height = config.height;
    }

    fn get_current_texture(&self, _token: private::Token) -> Option<Self::Target> {
        self.texture.as_ref().map(|t| HeadlessTarget {
            view: t.create_view(&TextureViewDescriptor::default()),
        })
    }
}

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct TextBgVertexMember {
    vertex: [f32; 2],
    bg_color: u32,
}

// Vertex + UVCoord + Color
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy)]
struct TextVertexMember {
    vertex: [f32; 2],
    uv: [f32; 2],
    fg_color: u32,
    underline_pos: u32,
    underline_color: u32,
}

pub(crate) struct TextCacheBgPipeline {
    pipeline: RenderPipeline,
    fs_uniforms: BindGroup,
}

pub(crate) struct TextCacheFgPipeline {
    pipeline: RenderPipeline,
    fs_uniforms: BindGroup,
    atlas_bindings: BindGroup,
}

pub(crate) struct WgpuState {
    _text_dest_view: TextureView,
}

fn c2c(color: ratatui::style::Color, reset: Rgb) -> Rgb {
    match color {
        Color::Reset => reset,
        Color::Black => BLACK,
        Color::Red => RED,
        Color::Green => GREEN,
        Color::Yellow => YELLOW,
        Color::Blue => BLUE,
        Color::Magenta => MAGENTA,
        Color::Cyan => CYAN,
        Color::Gray => GRAY,
        Color::DarkGray => DARKGRAY,
        Color::LightRed => LIGHTRED,
        Color::LightGreen => LIGHTGREEN,
        Color::LightYellow => LIGHTYELLOW,
        Color::LightBlue => LIGHTBLUE,
        Color::LightMagenta => LIGHTMAGENTA,
        Color::LightCyan => LIGHTCYAN,
        Color::White => WHITE,
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Indexed(idx) => ANSI_TO_RGB[idx as usize],
    }
}

pub(crate) fn build_wgpu_state(
    device: &Device,
    drawable_width: u32,
    drawable_height: u32,
) -> WgpuState {
    let text_dest = device.create_texture(&TextureDescriptor {
        label: Some("Text Compositor Out"),
        size: Extent3d {
            width: drawable_width.max(1),
            height: drawable_height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let text_dest_view = text_dest.create_view(&TextureViewDescriptor::default());

    WgpuState {
        _text_dest_view: text_dest_view,
    }
}
