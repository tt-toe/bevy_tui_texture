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

pub(crate) fn build_text_bg_compositor(device: &Device, format: TextureFormat) -> TextCacheBgPipeline {
    let bg_shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("BG Compositor Shader"),
        source: ShaderSource::Wgsl(include_str!("shaders/composite_bg.wgsl").into()),
    });

    // Bind group 0 (the screen-size uniform) is intentionally per-terminal,
    // not built here - screen size differs per terminal even when several
    // share this pipeline/atlas (IMPROVEMENT.md C3), so each `TerminalGpuState`
    // builds its own bind group against this layout (see
    // `TerminalGpuState::new`) instead of one being baked in at pipeline
    // creation.
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

    TextCacheBgPipeline { pipeline: bg_pipeline }
}

pub(crate) fn build_text_fg_compositor(
    device: &Device,
    atlas_size: &Buffer,
    cache: &TextureView,
    sampler: &Sampler,
    format: TextureFormat,
) -> TextCacheFgPipeline {
    let fg_shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("FG Compositor Shader"),
        source: ShaderSource::Wgsl(include_str!("shaders/composite_fg.wgsl").into()),
    });

    // Bind group 0 (the screen-size uniform) is per-terminal - see the
    // comment in `build_text_bg_compositor` above; the same reasoning
    // applies here, so only the layout is built in this function.
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

    // Binding 1 is intentionally a gap here (used to be the color-glyph
    // mask texture, removed - see IMPROVEMENT.md C1). wgpu does not
    // require bind group entries to be contiguous; leaving the gap keeps
    // this diff minimal and binding 2/3 unambiguous against the shader.
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
        atlas_bindings: fg_bind_group_1,
    }
}

use std::num::NonZeroU32;

use ratatui::style::Color;
use wgpu::BindGroup;
use wgpu::Device;
use wgpu::Extent3d;
use wgpu::RenderPipeline;
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
}

pub(crate) struct TextCacheFgPipeline {
    pipeline: RenderPipeline,
    atlas_bindings: BindGroup,
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

/// CPU-computed draw payload extracted from a dirty `Tui` each frame: the
/// background/foreground vertex data ratatui's diffed buffer produced this
/// draw, plus which font's shared atlas/pipelines to render it against.
/// Built by `BevyTerminalBackend::take_draw_payload` (main world), consumed
/// by `TerminalGpuState::render` (render world). Fields are crate-private -
/// callers outside `backend` only move this type around, they never need
/// to inspect it.
///
/// Carries no glyph rasterizations and no index buffer:
/// - Glyph uploads queue in the shared per-font CPU state
///   (`Fonts::with_shared_cpu_state`, IMPROVEMENT.md C3) instead of here,
///   and are drained once per font per frame via
///   `BevyTerminalBackend::take_shared_glyph_uploads` - not once per
///   terminal, since multiple terminals can share the same font and thus
///   the same atlas.
/// - Quad indices are a pure function of vertex count (quad `i` ->
///   `[4i, 4i+1, 4i+2, 4i+2, 4i+3, 4i+1]`), so `TerminalGpuState` derives
///   them from one static, grow-only index buffer instead of this payload
///   shipping a freshly rebuilt one every frame.
pub(crate) struct TerminalDrawPayload {
    /// Pixel dimensions used for the screen-size uniform - the *drawable*
    /// area (viewport insets already applied), not necessarily the full
    /// `cols * char_width_px` grid size.
    screen_width_px: f32,
    screen_height_px: f32,
    /// Color to clear to when both vertex `Vec`s are empty (nothing drawn
    /// yet, or mid-resize) - see `BevyTerminalBackend::initial_fill`.
    clear_color: [u8; 4],
    /// Identity of the `Fonts` this terminal renders with (see
    /// [`crate::fonts::Fonts::identity`]) - the render world uses this to
    /// find the correct shared atlas/pipelines (`SharedFontGpuStore`,
    /// IMPROVEMENT.md C3) for this terminal's vertex data.
    font_key: usize,
    /// Phase 2 partial redraw: `true` means this payload covers only the
    /// rows `BevyTerminalBackend::take_draw_payload` found dirty (each
    /// preceded by a synthesized row-clear quad), and the render pass must
    /// use `LoadOp::Load` to preserve every other row's existing pixels.
    /// `false` means this payload covers every row and the render pass
    /// uses `LoadOp::Clear` - required whenever the destination texture's
    /// current content can't be trusted (see
    /// `BevyTerminalBackend::full_redraw_needed`).
    load_previous: bool,
    bg_vertices: Vec<TextBgVertexMember>,
    text_vertices: Vec<TextVertexMember>,
}

impl TerminalDrawPayload {
    /// Drop the vertex geometry (drawn at whatever grid size was current at
    /// the time) - used by `Tui::apply_pending_resize`, where the
    /// just-taken payload's geometry was computed at the OLD grid size and
    /// would render garbled against the freshly resized destination
    /// texture. Also forces a full clear (`load_previous = false`): the
    /// destination texture is about to be recreated at the new size, so
    /// there is nothing valid for a `LoadOp::Load` to preserve.
    pub(crate) fn discard_stale_geometry(&mut self) {
        self.bg_vertices.clear();
        self.text_vertices.clear();
        self.load_previous = false;
    }

    /// Identity of the `Fonts` this terminal renders with - used by
    /// `render_tui_textures` (`bevy_plugin.rs`) to look up the correct
    /// [`SharedFontGpuState`] for this payload's vertex data.
    pub(crate) fn font_key(&self) -> usize {
        self.font_key
    }

    /// `true` iff this payload is a full redraw (`load_previous == false`).
    /// A `pub(crate)` accessor for tests outside the `backend` module -
    /// `load_previous` itself stays private since callers outside this
    /// module otherwise never need to inspect payload internals.
    #[cfg(test)]
    pub(crate) fn is_full(&self) -> bool {
        !self.load_previous
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

/// Starting capacity, in quads, for a terminal's index buffer (see
/// `TerminalGpuState::index_buffer`) - grown (doubling) the first time a
/// draw needs more. Covers a small static terminal without an immediate
/// regrow; large or busy terminals pay one regrow the first time they
/// exceed it, never again after that (grow-only, sized to the largest
/// frame ever drawn).
const INITIAL_INDEX_QUAD_CAPACITY: u32 = 64;

/// Builds the index pattern for `quad_count` independent quads: quad `i`
/// occupies vertices `[4i, 4i+1, 4i+2, 4i+3]` and is drawn as two
/// triangles `(4i, 4i+1, 4i+2)` + `(4i+2, 4i+3, 4i+1)`. Pure function of
/// the count - this is the entire reason the index buffer can be static
/// and grow-only instead of rebuilt from a `TerminalDrawPayload` every
/// frame (see IMPROVEMENT.md B2).
fn build_quad_indices(quad_count: u32) -> Vec<[u32; 6]> {
    (0..quad_count)
        .map(|i| {
            let base = i * 4;
            [base, base + 1, base + 2, base + 2, base + 3, base + 1]
        })
        .collect()
}

/// Grows a persistent GPU buffer (grow-only, doubling) if it doesn't
/// already cover `needed_bytes`, recreating it with the given `usage` and
/// `label`. Shared by every per-terminal buffer that is rewritten every
/// dirty frame but only needs to change *size* on the rare frame whose
/// content is bigger than anything seen before (`TerminalGpuState`'s
/// `bg_vertex_buffer`/`fg_vertex_buffer` - see IMPROVEMENT.md B1). Callers
/// still need to `queue.write_buffer` the actual bytes after this returns;
/// resizing alone does not upload anything.
fn ensure_buffer_capacity(
    device: &Device,
    buffer: &mut Buffer,
    capacity_bytes: &mut u64,
    needed_bytes: u64,
    usage: BufferUsages,
    label: &str,
) {
    if needed_bytes <= *capacity_bytes {
        return;
    }
    let new_capacity = needed_bytes.next_power_of_two();
    *buffer = device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size: new_capacity,
        mapped_at_creation: false,
        usage,
    });
    *capacity_bytes = new_capacity;
}

/// Render-world GPU resources shared by every terminal using the same
/// `Fonts` (IMPROVEMENT.md C3): the glyph atlas texture and the
/// background/foreground compositor pipelines (which bind to that same
/// atlas texture view, so they must live and die with it). Keyed by font
/// identity ([`crate::fonts::Fonts::identity`]) in the render-world store
/// (`bevy_plugin.rs`), not by destination image - two terminals sharing a
/// font share one 2048x2048 atlas and one pipeline pair instead of one
/// each. `target_format` mirrors whatever pixel format terminal
/// destination `GpuImage`s are actually created with (always
/// `Rgba8Unorm` today - see `TerminalTexture::create` - so in practice
/// every terminal's format matches regardless of font).
pub(crate) struct SharedFontGpuState {
    text_cache: Texture,
    text_bg_compositor: TextCacheBgPipeline,
    text_fg_compositor: TextCacheFgPipeline,
}

impl SharedFontGpuState {
    pub(crate) fn new(device: &Device, queue: &Queue, target_format: TextureFormat) -> Self {
        use wgpu::util::{BufferInitDescriptor, DeviceExt};
        use wgpu::{AddressMode, FilterMode, SamplerDescriptor};

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

        // The atlas texture starts as genuinely uninitialized GPU memory,
        // and glyph uploads only ever write small per-glyph sub-rectangles
        // (`upload_glyphs`, below). Without this, the FIRST partial
        // write/sample of an uninitialized subresource makes wgpu silently
        // perform its own full-texture "lazy initialization" clear to
        // satisfy the "reads see zero unless written" guarantee; on the
        // GL/WebGL2 backend that clear surfaces as a console WARN
        // ("texSubImage: Texture has not been initialized prior to a
        // partial upload" / "is incurring lazy initialization"). Explicitly
        // zeroing it up front here marks its subresources initialized in
        // wgpu's tracker, so that implicit clear - and its warning - never
        // triggers later. One-time cost per font (atlas creation, not per
        // frame, and shared by every terminal using this font).
        zero_init_texture(queue, &text_cache, CACHE_WIDTH, CACHE_HEIGHT, 4);

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Screen size varies per terminal (several terminals can share this
        // font/atlas but each has its own pixel dimensions), so that
        // uniform buffer - and its bind group - lives on `TerminalGpuState`
        // instead, built against this pipeline's bind group 0 layout (see
        // `TerminalGpuState::new`). Nothing screen-size-shaped is created
        // here.
        let atlas_size_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Atlas Size buffer"),
            contents: bytemuck::cast_slice(&[CACHE_WIDTH as f32, CACHE_HEIGHT as f32, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let text_bg_compositor = build_text_bg_compositor(device, target_format);
        let text_fg_compositor = build_text_fg_compositor(
            device,
            &atlas_size_buffer,
            &text_cache_view,
            &sampler,
            target_format,
        );

        Self {
            text_cache,
            text_bg_compositor,
            text_fg_compositor,
        }
    }

    /// Uploads pending glyph rasterizations (queued via
    /// `Fonts::with_shared_cpu_state`, potentially by several terminals
    /// sharing this font) to the shared atlas texture. Called once per
    /// font per frame, before rendering any terminal that uses it.
    pub(crate) fn upload_glyphs(
        &self,
        queue: &Queue,
        uploads: &[(crate::utils::text_atlas::CacheRect, Vec<u32>)],
    ) {
        for (cached, image) in uploads {
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
    }
}

/// Render-world GPU resources for one terminal: the screen-size uniform,
/// the quad index buffer, and the vertex buffers - everything that is
/// genuinely per-terminal rather than shared by font (see
/// [`SharedFontGpuState`] for the atlas texture and compositor pipelines,
/// IMPROVEMENT.md C3). Lazily created on a terminal's first extract (see
/// `bevy_plugin.rs`'s render-world store), keyed by destination image.
pub(crate) struct TerminalGpuState {
    text_screen_size_buffer: Buffer,
    text_screen_size_bind_group: BindGroup,
    /// Static, grow-only quad index buffer shared by the bg and fg passes
    /// each render (their draw calls take independent index *ranges* out
    /// of the same buffer - see `render`). Never rebuilt from scratch on a
    /// steady-state frame; only regenerated, at a larger size, the first
    /// time a frame needs more quads than `index_buffer_quad_capacity`.
    index_buffer: Buffer,
    index_buffer_quad_capacity: u32,
    /// Persistent, grow-only vertex buffers - `queue.write_buffer`d with
    /// this frame's data every dirty frame instead of recreated via
    /// `create_buffer_init` (see IMPROVEMENT.md B1). Only resized (via
    /// `ensure_buffer_capacity`) the first time a frame's data exceeds the
    /// current capacity.
    bg_vertex_buffer: Buffer,
    bg_vertex_buffer_capacity_bytes: u64,
    fg_vertex_buffer: Buffer,
    fg_vertex_buffer_capacity_bytes: u64,
}

impl TerminalGpuState {
    /// `shared`'s pipelines determine this terminal's screen-size bind
    /// group layout (both compositors take the same layout for bind group
    /// 0 - see `build_text_bg_compositor`/`build_text_fg_compositor`), so
    /// this terminal's OWN screen-size buffer needs a bind group built
    /// against that layout, distinct from `shared`'s own (placeholder)
    /// one. Terminals sharing `shared` each get their own such bind group,
    /// pointing at their own buffer.
    pub(crate) fn new(device: &Device, shared: &SharedFontGpuState) -> Self {
        use wgpu::util::{BufferInitDescriptor, DeviceExt};
        use wgpu::{BufferDescriptor, BufferUsages};

        let text_screen_size_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Text Uniforms Buffer"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let text_screen_size_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Terminal Screen Size Bind Group"),
            layout: &shared.text_bg_compositor.pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: text_screen_size_buffer.as_entire_binding(),
            }],
        });

        let index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Terminal Quad Indices"),
            contents: bytemuck::cast_slice(&build_quad_indices(INITIAL_INDEX_QUAD_CAPACITY)),
            usage: BufferUsages::INDEX,
        });

        let bg_vertex_buffer_capacity_bytes =
            INITIAL_INDEX_QUAD_CAPACITY as u64 * 4 * std::mem::size_of::<TextBgVertexMember>() as u64;
        let bg_vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Text Bg Vertices"),
            size: bg_vertex_buffer_capacity_bytes,
            mapped_at_creation: false,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });

        let fg_vertex_buffer_capacity_bytes =
            INITIAL_INDEX_QUAD_CAPACITY as u64 * 4 * std::mem::size_of::<TextVertexMember>() as u64;
        let fg_vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Text Vertices"),
            size: fg_vertex_buffer_capacity_bytes,
            mapped_at_creation: false,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });

        Self {
            text_screen_size_buffer,
            text_screen_size_bind_group,
            index_buffer,
            index_buffer_quad_capacity: INITIAL_INDEX_QUAD_CAPACITY,
            bg_vertex_buffer,
            bg_vertex_buffer_capacity_bytes,
            fg_vertex_buffer,
            fg_vertex_buffer_capacity_bytes,
        }
    }

    /// Grows `index_buffer` (doubling) if it doesn't already cover
    /// `needed_quads`. The index pattern is a pure function of quad count
    /// (see `build_quad_indices`), so regenerating it at a larger size is
    /// all growth ever needs - there is no shrink path; a terminal's
    /// largest-ever frame sets the buffer size for the rest of its
    /// lifetime.
    fn ensure_index_capacity(&mut self, device: &Device, needed_quads: u32) {
        if needed_quads <= self.index_buffer_quad_capacity {
            return;
        }
        use wgpu::util::{BufferInitDescriptor, DeviceExt};

        let new_capacity = needed_quads.next_power_of_two();
        self.index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Terminal Quad Indices"),
            contents: bytemuck::cast_slice(&build_quad_indices(new_capacity)),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.index_buffer_quad_capacity = new_capacity;
    }

    /// Records the background + foreground passes into `target`, against
    /// `shared`'s atlas/pipelines. Glyph uploads are NOT handled here -
    /// `SharedFontGpuState::upload_glyphs` must be called once per font
    /// per frame before rendering any terminal sharing that font (see
    /// `render_tui_textures` in `bevy_plugin.rs`).
    ///
    /// Phase 2 partial redraw: `draw.load_previous` selects `LoadOp::Load`
    /// (preserve every pixel not touched by this payload's quads - a
    /// partial payload, whose dirty rows already carry a synthesized
    /// full-row clear quad from `take_draw_payload`) or `LoadOp::Clear`
    /// (wipe the whole texture first - a full payload).
    pub(crate) fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        shared: &SharedFontGpuState,
        encoder: &mut wgpu::CommandEncoder,
        target: &TextureView,
        draw: &TerminalDrawPayload,
    ) {
        use wgpu::{
            BufferUsages, IndexFormat, LoadOp, Operations, RenderPassColorAttachment,
            RenderPassDescriptor, StoreOp,
        };

        // Only meaningful when `load_previous` is false (a `LoadOp::Load`
        // pass ignores the clear color entirely) - previously the
        // vertices-exist branch cleared to hardcoded black while only the
        // empty branch cleared to `initial_fill`, so a terminal with a
        // non-black `initial_fill` flashed the wrong color on its very
        // first (content-free) frame. Also what `flush()`'s bg-quad
        // skipping compares against (`initial_fill_u32` there), so a
        // skipped quad and an actual clear must produce the same pixels.
        let [r, g, b, a] = draw.clear_color;
        let clear_color = wgpu::Color {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
            a: a as f64 / 255.0,
        };
        let load = if draw.load_previous {
            LoadOp::Load
        } else {
            LoadOp::Clear(clear_color)
        };

        // Branch on EITHER vertex `Vec`, not `text_vertices` alone: a
        // partial payload can legitimately carry bg-only content (a row's
        // text was deleted, leaving just its synthesized clear quad) -
        // branching on `text_vertices` alone would fall into the
        // clear-only branch below and, under `LoadOp::Clear`, wipe rows
        // this payload never touched. (Both empty only happens for a
        // partial payload with zero dirty rows, which the live pipeline
        // never produces - `Tui::flush` only calls `take_draw_payload`
        // when at least one row was marked dirty - but `load` still
        // resolves that case correctly too: a `LoadOp::Load` pass with no
        // draw calls is a no-op.)
        if !draw.bg_vertices.is_empty() || !draw.text_vertices.is_empty() {
            queue.write_buffer(
                &self.text_screen_size_buffer,
                0,
                bytemuck::cast_slice(&[draw.screen_width_px, draw.screen_height_px, 0.0, 0.0]),
            );

            let bg_quads = draw.bg_vertices.len() as u32 / 4;
            let fg_quads = draw.text_vertices.len() as u32 / 4;
            self.ensure_index_capacity(device, bg_quads.max(fg_quads));

            // Persistent, grow-only vertex buffers (IMPROVEMENT.md B1):
            // `write_buffer` this frame's bytes at offset 0 instead of
            // creating a fresh buffer every dirty frame. A buffer whose
            // capacity is larger than this frame's data is fine as-is -
            // the draw calls below only ever read vertex indices within
            // `0..quads*4` (via the index buffer), so stale bytes past the
            // current frame's data in an oversized buffer are never
            // sampled.
            let bg_bytes = bytemuck::cast_slice::<_, u8>(&draw.bg_vertices).len() as u64;
            ensure_buffer_capacity(
                device,
                &mut self.bg_vertex_buffer,
                &mut self.bg_vertex_buffer_capacity_bytes,
                bg_bytes,
                BufferUsages::VERTEX | BufferUsages::COPY_DST,
                "Text Bg Vertices",
            );
            if bg_bytes > 0 {
                queue.write_buffer(&self.bg_vertex_buffer, 0, bytemuck::cast_slice(&draw.bg_vertices));
            }

            let fg_bytes = bytemuck::cast_slice::<_, u8>(&draw.text_vertices).len() as u64;
            ensure_buffer_capacity(
                device,
                &mut self.fg_vertex_buffer,
                &mut self.fg_vertex_buffer_capacity_bytes,
                fg_bytes,
                BufferUsages::VERTEX | BufferUsages::COPY_DST,
                "Text Vertices",
            );
            if fg_bytes > 0 {
                queue.write_buffer(&self.fg_vertex_buffer, 0, bytemuck::cast_slice(&draw.text_vertices));
            }

            let mut text_render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Terminal Text Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: Operations {
                        load,
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });

            // Both passes draw out of the same static index buffer - it's
            // large enough for the larger of the two quad counts (just
            // ensured above), and each pass only ever draws its own
            // `0..quads*6` range, so the shared buffer never mixes bg and
            // fg indices.
            text_render_pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint32);

            text_render_pass.set_pipeline(&shared.text_bg_compositor.pipeline);
            text_render_pass.set_bind_group(0, &self.text_screen_size_bind_group, &[]);
            text_render_pass.set_vertex_buffer(0, self.bg_vertex_buffer.slice(..));
            text_render_pass.draw_indexed(0..bg_quads * 6, 0, 0..1);

            text_render_pass.set_pipeline(&shared.text_fg_compositor.pipeline);
            text_render_pass.set_bind_group(0, &self.text_screen_size_bind_group, &[]);
            text_render_pass.set_bind_group(1, &shared.text_fg_compositor.atlas_bindings, &[]);
            text_render_pass.set_vertex_buffer(0, self.fg_vertex_buffer.slice(..));
            text_render_pass.draw_indexed(0..fg_quads * 6, 0, 0..1);
        } else {
            let _clear_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Terminal Clear Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: Operations {
                        load,
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
        }
    }
}
