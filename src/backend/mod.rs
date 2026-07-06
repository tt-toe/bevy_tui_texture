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
//! - **Text Atlas** - GPU texture cache for rendered glyphs (2048x2048px,
//!   the WebGL2-safe max texture dimension, square to maximize capacity)
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
/// Square, and pinned to exactly `wgpu::Limits::downlevel_webgl2_defaults()`'s
/// `max_texture_dimension_2d` (2048) - the guaranteed-safe ceiling for
/// `wgpu::Device::create_texture` on WebGL2/GLES3/D3D11-class hardware. This
/// is the same conservative limit set `retro_crt.rs` opts into via
/// `WgpuSettingsPriority::WebGL2` for its wasm32 build. Unlike the
/// per-terminal destination `Image` (`cols * char_width_px` x
/// `rows * char_height_px` - its aspect ratio is dictated by the caller's
/// grid and must not be distorted), the atlas has no meaningful "correct"
/// aspect ratio: it is just a bin-packed cache, so making it square
/// maximizes glyph capacity for a given max-dimension budget instead of
/// leaving area on the table the way a non-square-but-under-the-cap size
/// would. Same value on every platform: staying under the WebGL2 ceiling by
/// construction means the atlas never has a wasm-only failure mode to begin
/// with.
pub(crate) const CACHE_WIDTH: u32 = 2048;

/// Height of the glyph cache texture in pixels. See [`CACHE_WIDTH`].
pub(crate) const CACHE_HEIGHT: u32 = 2048;

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
        bind_group_layouts: &[Some(&bg_bind_group_layout)],
        immediate_size: 0,
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
        multiview_mask: None,
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
        bind_group_layouts: &[Some(&fg_bind_group_layout_0), Some(&fg_bind_group_layout_1)],
        immediate_size: 0,
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
        multiview_mask: None,
        cache: None,
    });

    TextCacheFgPipeline {
        pipeline: fg_pipeline,
        fs_uniforms: fg_bind_group_0,
        atlas_bindings: fg_bind_group_1,
    }
}

use std::num::NonZeroU32;

use tracing::error;
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
        // wgpu 29: get_current_texture returns an enum instead of a Result.
        let output = match self.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            status => {
                error!("surface texture unavailable: {status:?}");
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

/// CPU-computed draw payload extracted from a dirty `Tui` each frame:
/// pending glyph rasterizations to upload into the atlas texture, plus the
/// background/foreground vertex+index data ratatui's diffed buffer produced
/// this draw. Built by `BevyTerminalBackend::take_draw_payload` (main
/// world), consumed by `TerminalGpuState::render` (render world). Fields
/// are crate-private - callers outside `backend` only move this type
/// around, they never need to inspect it.
pub(crate) struct TerminalDrawPayload {
    /// Pixel dimensions used for the screen-size uniform - the *drawable*
    /// area (viewport insets already applied), not necessarily the full
    /// `cols * char_width_px` grid size.
    screen_width_px: f32,
    screen_height_px: f32,
    /// Color to clear to when `text_vertices` is empty (nothing drawn yet,
    /// or mid-resize) - see `BevyTerminalBackend::initial_fill`.
    clear_color: [u8; 4],
    glyph_uploads: Vec<(crate::utils::text_atlas::CacheRect, Vec<u32>, bool)>,
    bg_vertices: Vec<TextBgVertexMember>,
    text_vertices: Vec<TextVertexMember>,
    text_indices: Vec<[u32; 6]>,
}

impl TerminalDrawPayload {
    /// Fold an older, never-rendered payload into this (newer) one. The
    /// vertex/index data is full-frame state, so the newer set simply wins -
    /// but `glyph_uploads` are one-shot atlas writes the CPU-side `Atlas`
    /// LRU already considers cached: dropping one leaves its atlas slot as
    /// garbage *forever* (the glyph is never re-rasterized), which shows up
    /// as permanently missing characters. Whenever a payload is about to be
    /// replaced before the render world consumed it (`Tui::flush` on a
    /// dirty-every-frame terminal while the renderer is still initializing,
    /// or `extract_tui_draws` overwriting an entry whose destination
    /// `GpuImage` isn't prepared yet), its uploads must be carried forward
    /// through this method, never discarded.
    ///
    /// The older uploads are ordered *before* this payload's own: if an LRU
    /// slot was reused in between, the later write is the live one and must
    /// land last.
    pub(crate) fn merge_undelivered(&mut self, older: TerminalDrawPayload) {
        if older.glyph_uploads.is_empty() {
            return;
        }
        tracing::debug!(
            "carrying forward {} undelivered glyph upload(s) from a superseded draw payload",
            older.glyph_uploads.len()
        );
        let mut uploads = older.glyph_uploads;
        uploads.append(&mut self.glyph_uploads);
        self.glyph_uploads = uploads;
    }

    /// Drop the vertex/index geometry (drawn at whatever grid size was
    /// current at the time) while keeping `glyph_uploads` - used by
    /// `Tui::apply_pending_resize`, where the just-taken payload's geometry
    /// was computed at the OLD grid size and would render garbled against
    /// the freshly resized destination texture, but its glyph-atlas uploads
    /// remain valid pixel data regardless of grid size.
    pub(crate) fn discard_stale_geometry(&mut self) {
        self.bg_vertices.clear();
        self.text_vertices.clear();
        self.text_indices.clear();
    }
}

/// Zeroes an entire freshly created 2D texture via one full-extent
/// `queue.write_texture` call, marking every subresource "initialized" in
/// wgpu's tracker up front. See the call sites in `TerminalGpuState::new`
/// for why this matters (avoiding wgpu's implicit lazy-init clear, and the
/// WebGL console warning it produces, on the first partial write/sample).
fn zero_init_texture(queue: &Queue, texture: &Texture, width: u32, height: u32, bytes_per_pixel: u32) {
    let zeros = vec![0u8; (width * height * bytes_per_pixel) as usize];
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &zeros,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_pixel),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Render-world GPU resources for one terminal: glyph atlas texture,
/// background/foreground compositor pipelines, and the screen-size uniform
/// buffer. Lazily created on a terminal's first extract (see
/// `bevy_plugin.rs`'s render-world store), keyed by destination image -
/// `target_format` mirrors whatever pixel format the destination `GpuImage`
/// was actually created with, so this never has to guess.
pub(crate) struct TerminalGpuState {
    text_cache: Texture,
    #[allow(dead_code)]
    text_mask: Texture,
    text_bg_compositor: TextCacheBgPipeline,
    text_fg_compositor: TextCacheFgPipeline,
    text_screen_size_buffer: Buffer,
    #[allow(dead_code)]
    wgpu_state: WgpuState,
}

impl TerminalGpuState {
    pub(crate) fn new(
        device: &Device,
        queue: &Queue,
        target_format: TextureFormat,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Self {
        use wgpu::util::{BufferInitDescriptor, DeviceExt};
        use wgpu::{
            AddressMode, BufferDescriptor, BufferUsages, FilterMode, SamplerDescriptor,
        };

        let text_cache = device.create_texture(&TextureDescriptor {
            label: Some("Text Cache"),
            size: Extent3d {
                width: CACHE_WIDTH,
                height: CACHE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let text_cache_view = text_cache.create_view(&TextureViewDescriptor::default());

        let text_mask = device.create_texture(&TextureDescriptor {
            label: Some("Text Mask"),
            size: Extent3d {
                width: CACHE_WIDTH,
                height: CACHE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let text_mask_view = text_mask.create_view(&TextureViewDescriptor::default());

        // Both atlas textures start as genuinely uninitialized GPU memory,
        // and glyph uploads only ever write small per-glyph sub-rectangles
        // (`render`, below) - `text_mask` currently isn't written at all.
        // Without this, the FIRST partial write/sample of an uninitialized
        // subresource makes wgpu silently perform its own full-texture
        // "lazy initialization" clear to satisfy the "reads see zero unless
        // written" guarantee; on the GL/WebGL2 backend that clear surfaces
        // as a console WARN ("texSubImage: Texture has not been
        // initialized prior to a partial upload" / "is incurring lazy
        // initialization"). Explicitly zeroing both textures up front here
        // marks their subresources initialized in wgpu's tracker, so that
        // implicit clear - and its warning - never triggers later. One-time
        // cost per `Tui` (atlas creation, not per frame).
        zero_init_texture(queue, &text_cache, CACHE_WIDTH, CACHE_HEIGHT, 4);
        zero_init_texture(queue, &text_mask, CACHE_WIDTH, CACHE_HEIGHT, 1);

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let text_screen_size_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Text Uniforms Buffer"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let atlas_size_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Atlas Size buffer"),
            contents: bytemuck::cast_slice(&[CACHE_WIDTH as f32, CACHE_HEIGHT as f32, 0.0, 0.0]),
            usage: BufferUsages::UNIFORM,
        });

        let text_bg_compositor =
            build_text_bg_compositor(device, &text_screen_size_buffer, target_format);
        let text_fg_compositor = build_text_fg_compositor(
            device,
            &text_screen_size_buffer,
            &atlas_size_buffer,
            &text_cache_view,
            &text_mask_view,
            &sampler,
            target_format,
        );

        let wgpu_state = build_wgpu_state(device, pixel_width, pixel_height);

        Self {
            text_cache,
            text_mask,
            text_bg_compositor,
            text_fg_compositor,
            text_screen_size_buffer,
            wgpu_state,
        }
    }

    /// Uploads pending glyph rasterizations to the atlas texture, then
    /// records the background + foreground passes into `target`. Mirrors
    /// the pre-Phase-B `BevyTerminalBackend::render_to_texture`, operating
    /// on an extracted [`TerminalDrawPayload`] instead of backend fields.
    pub(crate) fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        target: &TextureView,
        draw: &TerminalDrawPayload,
    ) {
        use wgpu::util::{BufferInitDescriptor, DeviceExt};
        use wgpu::{
            BufferUsages, CommandEncoderDescriptor, IndexFormat, LoadOp, Operations,
            RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("Terminal Draw Encoder"),
        });

        for (cached, image, _is_emoji) in &draw.glyph_uploads {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.text_cache,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: cached.x,
                        y: cached.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(image),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(cached.width * std::mem::size_of::<u32>() as u32),
                    rows_per_image: Some(cached.height),
                },
                Extent3d {
                    width: cached.width,
                    height: cached.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        if !draw.text_vertices.is_empty() {
            queue.write_buffer(
                &self.text_screen_size_buffer,
                0,
                bytemuck::cast_slice(&[draw.screen_width_px, draw.screen_height_px, 0.0, 0.0]),
            );

            let bg_vertices = device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Bg Vertices"),
                contents: bytemuck::cast_slice(&draw.bg_vertices),
                usage: BufferUsages::VERTEX,
            });
            let fg_vertices = device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Vertices"),
                contents: bytemuck::cast_slice(&draw.text_vertices),
                usage: BufferUsages::VERTEX,
            });
            let indices = device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Indices"),
                contents: bytemuck::cast_slice(&draw.text_indices),
                usage: BufferUsages::INDEX,
            });

            let mut text_render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Terminal Text Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::BLACK),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });

            text_render_pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);

            text_render_pass.set_pipeline(&self.text_bg_compositor.pipeline);
            text_render_pass.set_bind_group(0, &self.text_bg_compositor.fs_uniforms, &[]);
            text_render_pass.set_vertex_buffer(0, bg_vertices.slice(..));
            text_render_pass.draw_indexed(0..(draw.bg_vertices.len() as u32 / 4) * 6, 0, 0..1);

            text_render_pass.set_pipeline(&self.text_fg_compositor.pipeline);
            text_render_pass.set_bind_group(0, &self.text_fg_compositor.fs_uniforms, &[]);
            text_render_pass.set_bind_group(1, &self.text_fg_compositor.atlas_bindings, &[]);
            text_render_pass.set_vertex_buffer(0, fg_vertices.slice(..));
            text_render_pass.draw_indexed(0..(draw.text_vertices.len() as u32 / 4) * 6, 0, 0..1);
        } else {
            let [r, g, b, a] = draw.clear_color;
            let clear_color = wgpu::Color {
                r: r as f64 / 255.0,
                g: g as f64 / 255.0,
                b: b as f64 / 255.0,
                a: a as f64 / 255.0,
            };
            let _clear_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Terminal Clear Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(clear_color),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
        }

        queue.submit(Some(encoder.finish()));
    }
}
