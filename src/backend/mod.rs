//! WGPU-based ratatui backend implementation.

pub mod bevy_backend;
pub(crate) mod glyph_constraint;
pub(crate) mod programmatic_glyphs;
pub(crate) mod rasterize;

/// Width of the glyph cache texture in pixels.
pub(crate) const CACHE_WIDTH: u32 = 2048;

/// Height of the glyph cache texture in pixels.
pub(crate) const CACHE_HEIGHT: u32 = 2048;

use std::num::NonZeroU32;

use ratatui::style::Color;
use wgpu::*;

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

/// One glyph (or one synthesized row-clear rectangle), as a GPU instance.
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellInstance {
    /// Destination rectangle in the terminal texture: `[x, y, width, height]`.
    pub(crate) rect: [u16; 4],
    /// Top-left texel of the glyph's atlas slot (the slot is sampled 1:1,
    /// `rect`'s width/height in size).
    pub(crate) uv: [u16; 2],
    /// Foreground color, RGBA.
    pub(crate) fg: [u8; 4],
    /// Background color, RGBA - written as-is wherever the glyph has no
    /// coverage (alpha 0 is how `transparent_reset_bg` shows through).
    pub(crate) bg: [u8; 4],
}

const CELL_INSTANCE_ATTRIBUTES: [VertexAttribute; 4] =
    vertex_attr_array![0 => Uint16x4, 1 => Uint16x2, 2 => Unorm8x4, 3 => Unorm8x4];

/// The two pipelines sharing `shaders/composite.wgsl` and one instance
/// buffer layout: `cells` writes every glyph's cell outright (background
/// and glyph resolved in the fragment shader, `BlendState::REPLACE`);
/// `overlays` alpha-blends the rare extra glyph of a cell that an earlier
/// instance already covered (a combining mark shaped as its own glyph).
pub(crate) struct CompositorPipelines {
    cells: RenderPipeline,
    overlays: RenderPipeline,
    /// Atlas texture + sampler (bind group 1) - shared by every terminal
    /// using this font.
    atlas_bindings: BindGroup,
}

fn build_compositor(
    device: &Device,
    atlas: &TextureView,
    sampler: &Sampler,
    format: TextureFormat,
) -> CompositorPipelines {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("Terminal Compositor Shader"),
        source: ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
    });

    let globals_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("Terminal Globals Layout"),
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

    let atlas_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("Terminal Atlas Layout"),
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
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let atlas_bindings = device.create_bind_group(&BindGroupDescriptor {
        label: Some("Terminal Atlas Bindings"),
        layout: &atlas_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(atlas),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
        ],
    });

    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("Terminal Compositor Layout"),
        bind_group_layouts: &[Some(&globals_layout), Some(&atlas_layout)],
        immediate_size: 0,
    });

    let pipeline = |label, fragment_entry, blend| {
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: std::mem::size_of::<CellInstance>() as BufferAddress,
                    step_mode: VertexStepMode::Instance,
                    attributes: &CELL_INSTANCE_ATTRIBUTES,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some(fragment_entry),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    };

    CompositorPipelines {
        cells: pipeline("Terminal Cell Pipeline", "fs_cell", BlendState::REPLACE),
        overlays: pipeline(
            "Terminal Overlay Pipeline",
            "fs_overlay",
            BlendState::ALPHA_BLENDING,
        ),
        atlas_bindings,
    }
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
/// cell instances ratatui's diffed buffer produced this draw, plus which
/// font's shared atlas/pipelines to render them against.
pub(crate) struct TerminalDrawPayload {
    /// Pixel dimensions of the *drawable* area (viewport insets already
    /// applied), not necessarily the full `cols * char_width_px` grid size.
    screen_width_px: f32,
    screen_height_px: f32,
    /// Color to clear to on a full redraw - see
    /// `BevyTerminalBackend::initial_fill`.
    clear_color: [u8; 4],
    /// Identity of the `Fonts` this terminal renders with (see
    /// [`crate::fonts::Fonts::identity`]) - selects the shared
    /// atlas/pipelines (`SharedFontGpuStore`) in the render world.
    font_key: usize,
    /// Partial redraw: `true` means this payload covers only the rows
    /// `take_draw_payload` found dirty (each preceded by a row-clear
    /// instance) and the render pass must `LoadOp::Load` to preserve every
    /// other row.
    load_previous: bool,
    /// `instances[..cell_count]` are drawn by the cell pipeline (REPLACE),
    /// the rest by the overlay pipeline (alpha-blended on top) - bar the
    /// color range, see `color_count`.
    instances: Vec<CellInstance>,
    cell_count: u32,
    /// `instances[cell_count..cell_count + color_count]` are drawn by the
    /// color pipeline (emoji) in between: `[cells][colors][overlays]`.
    #[cfg(feature = "emoji")]
    color_count: u32,
}

impl TerminalDrawPayload {
    /// Drop the geometry (computed at the OLD grid size) - used by
    /// `Tui::apply_pending_resize`, where rendering it against the freshly
    /// resized destination texture would show garbled content.
    pub(crate) fn discard_stale_geometry(&mut self) {
        self.instances.clear();
        self.cell_count = 0;
        #[cfg(feature = "emoji")]
        {
            self.color_count = 0;
        }
        self.load_previous = false;
    }

    /// Identity of the `Fonts` this terminal renders with - used by
    /// `render_tui_textures` (`bevy_plugin.rs`) to look up the correct
    /// [`SharedFontGpuState`].
    pub(crate) fn font_key(&self) -> usize {
        self.font_key
    }

    /// `true` iff this payload is a full redraw (`load_previous == false`).
    #[cfg(test)]
    pub(crate) fn is_full(&self) -> bool {
        !self.load_previous
    }

    /// Instances drawn by the cell pipeline (one per covered cell).
    #[cfg(test)]
    pub(crate) fn cells(&self) -> &[CellInstance] {
        &self.instances[..self.cell_count as usize]
    }

    /// Instances drawn by the color pipeline (emoji).
    #[cfg(all(test, feature = "emoji"))]
    pub(crate) fn colors(&self) -> &[CellInstance] {
        let start = self.cell_count as usize;
        &self.instances[start..start + self.color_count as usize]
    }

    /// Instances drawn by the overlay pipeline (extra glyphs per cell).
    #[cfg(test)]
    pub(crate) fn overlays(&self) -> &[CellInstance] {
        #[cfg(feature = "emoji")]
        let start = (self.cell_count + self.color_count) as usize;
        #[cfg(not(feature = "emoji"))]
        let start = self.cell_count as usize;
        &self.instances[start..]
    }
}

/// Zeroes an entire freshly created single-channel 2D texture via one
/// full-extent `queue.write_texture` call, marking every subresource
/// "initialized" in wgpu's tracker up front (see the call site in
/// `SharedFontGpuState::new`).
fn zero_init_r8_texture(queue: &Queue, texture: &Texture, width: u32, height: u32) {
    let zeros = vec![0u8; (width * height) as usize];
    queue.write_texture(
        texture.as_image_copy(),
        &zeros,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Starting capacity, in instances, of a terminal's instance buffer - grown
/// (doubling) the first time a draw needs more, never shrunk.
const INITIAL_INSTANCE_CAPACITY: u64 = 256;

/// Render-world GPU resources shared by every terminal using the same
/// `Fonts`: the glyph atlas texture and the compositor pipelines (which
/// bind that atlas, so they live and die with it).
pub(crate) struct SharedFontGpuState {
    text_cache: Texture,
    compositor: CompositorPipelines,
    /// Color glyph atlas and pipeline - created when the first color glyph
    /// upload arrives, so nothing is built (or compiled, on WebGL2) until an
    /// emoji is drawn.
    #[cfg(feature = "emoji")]
    color: Option<ColorGpu>,
    #[cfg(feature = "emoji")]
    target_format: TextureFormat,
}

/// The color glyph atlas (premultiplied `Rgba8Unorm`, read by texel
/// coordinate - no sampler) and the third pipeline, which draws from it.
#[cfg(feature = "emoji")]
struct ColorGpu {
    texture: Texture,
    pipeline: RenderPipeline,
    bind_group: BindGroup,
}

impl SharedFontGpuState {
    pub(crate) fn new(device: &Device, queue: &Queue, target_format: TextureFormat) -> Self {
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
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let text_cache_view = text_cache.create_view(&TextureViewDescriptor::default());

        // The atlas starts as uninitialized GPU memory and glyph uploads
        // only ever write small sub-rectangles (`upload_glyphs`). Without
        // this, the first partial write makes wgpu perform its own
        // full-texture lazy-initialization clear, which on GL/WebGL2
        // surfaces as a console WARN ("texSubImage: Texture has not been
        // initialized prior to a partial upload"). Zeroing it up front
        // marks it initialized so that never triggers. One-time cost per
        // font, shared by every terminal using it.
        zero_init_r8_texture(queue, &text_cache, CACHE_WIDTH, CACHE_HEIGHT);

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let compositor = build_compositor(device, &text_cache_view, &sampler, target_format);

        Self {
            text_cache,
            compositor,
            #[cfg(feature = "emoji")]
            color: None,
            #[cfg(feature = "emoji")]
            target_format,
        }
    }

    #[cfg(feature = "emoji")]
    fn build_color(&self, device: &Device, queue: &Queue, (width, height): (u32, u32)) -> ColorGpu {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Color Glyph Cache"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &vec![0u8; (width * height * 4) as usize],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&TextureViewDescriptor::default());

        let atlas_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Color Atlas Layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Color Atlas Bindings"),
            layout: &atlas_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            }],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Terminal Color Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/composite_color.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Terminal Color Layout"),
            bind_group_layouts: &[
                Some(&self.compositor.cells.get_bind_group_layout(0)),
                Some(&atlas_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Terminal Color Pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_color"),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: std::mem::size_of::<CellInstance>() as BufferAddress,
                    step_mode: VertexStepMode::Instance,
                    attributes: &CELL_INSTANCE_ATTRIBUTES,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_color"),
                targets: &[Some(ColorTargetState {
                    format: self.target_format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        ColorGpu {
            texture,
            pipeline,
            bind_group,
        }
    }

    /// Uploads pending glyph coverage masks (queued via
    /// `Fonts::with_shared_cpu_state`, potentially by several terminals
    /// sharing this font) to the shared atlas texture.
    pub(crate) fn upload_glyphs(
        &self,
        queue: &Queue,
        uploads: &[(crate::utils::text_atlas::CacheRect, Vec<u8>)],
    ) {
        for (cached, mask) in uploads {
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &self.text_cache,
                    mip_level: 0,
                    origin: Origin3d {
                        x: cached.x,
                        y: cached.y,
                        z: 0,
                    },
                    aspect: TextureAspect::All,
                },
                mask,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(cached.width),
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

    /// [`Self::upload_glyphs`] for the color atlas (`size` is its texture
    /// size, from the CPU side), creating the texture and pipeline on the
    /// first call.
    #[cfg(feature = "emoji")]
    pub(crate) fn upload_color_glyphs(
        &mut self,
        device: &Device,
        queue: &Queue,
        size: (u32, u32),
        uploads: &[(crate::utils::text_atlas::CacheRect, Vec<u8>)],
    ) {
        if self.color.is_none() {
            self.color = Some(self.build_color(device, queue, size));
        }
        let Some(color) = &self.color else { return };
        for (cached, image) in uploads {
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &color.texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: cached.x,
                        y: cached.y,
                        z: 0,
                    },
                    aspect: TextureAspect::All,
                },
                image,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(cached.width * 4),
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

/// Render-world GPU resources for one terminal: the globals uniform and the
/// instance buffer - everything genuinely per-terminal rather than shared
/// by font (see [`SharedFontGpuState`]).
pub(crate) struct TerminalGpuState {
    globals_buffer: Buffer,
    globals_bind_group: BindGroup,
    /// What `globals_buffer` currently holds - it only changes on a resize,
    /// so a steady-state frame skips the uniform upload entirely.
    globals: [f32; 4],
    /// Persistent, grow-only instance buffer, `write_buffer`d in place
    /// every dirty frame.
    instance_buffer: Buffer,
    instance_capacity: u64,
}

impl TerminalGpuState {
    pub(crate) fn new(device: &Device, shared: &SharedFontGpuState) -> Self {
        let globals_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal Globals"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let globals_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Terminal Globals Bind Group"),
            layout: &shared.compositor.cells.get_bind_group_layout(0),
            entries: &[BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        Self {
            globals_buffer,
            globals_bind_group,
            globals: [0.0; 4],
            instance_buffer: Self::create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY),
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
        }
    }

    fn create_instance_buffer(device: &Device, capacity: u64) -> Buffer {
        device.create_buffer(&BufferDescriptor {
            label: Some("Terminal Cell Instances"),
            size: capacity * std::mem::size_of::<CellInstance>() as u64,
            mapped_at_creation: false,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        })
    }

    /// Records this terminal's draw into `target`, against `shared`'s
    /// atlas/pipelines.
    pub(crate) fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        shared: &SharedFontGpuState,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        draw: &TerminalDrawPayload,
    ) {
        let [r, g, b, a] = draw.clear_color;
        let load = if draw.load_previous {
            LoadOp::Load
        } else {
            LoadOp::Clear(wgpu::Color {
                r: r as f64 / 255.0,
                g: g as f64 / 255.0,
                b: b as f64 / 255.0,
                a: a as f64 / 255.0,
            })
        };

        let total = draw.instances.len() as u64;
        if total > 0 {
            let globals = [
                draw.screen_width_px,
                draw.screen_height_px,
                CACHE_WIDTH as f32,
                CACHE_HEIGHT as f32,
            ];
            if globals != self.globals {
                queue.write_buffer(&self.globals_buffer, 0, bytemuck::cast_slice(&globals));
                self.globals = globals;
            }

            if total > self.instance_capacity {
                self.instance_capacity = total.next_power_of_two();
                self.instance_buffer = Self::create_instance_buffer(device, self.instance_capacity);
            }
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&draw.instances),
            );
        }

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("Terminal Render Pass"),
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
        if total == 0 {
            return;
        }

        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_bind_group(1, &shared.compositor.atlas_bindings, &[]);

        let cells = draw.cell_count;
        if cells > 0 {
            pass.set_pipeline(&shared.compositor.cells);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            pass.draw(0..4, 0..cells);
        }
        #[cfg(feature = "emoji")]
        let colors = draw.color_count;
        #[cfg(not(feature = "emoji"))]
        let colors = 0u32;
        #[cfg(feature = "emoji")]
        if colors > 0
            && let Some(color) = &shared.color
        {
            let offset = cells as u64 * std::mem::size_of::<CellInstance>() as u64;
            pass.set_pipeline(&color.pipeline);
            pass.set_bind_group(1, &color.bind_group, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(offset..));
            pass.draw(0..4, 0..colors);
            pass.set_bind_group(1, &shared.compositor.atlas_bindings, &[]);
        }
        let overlays = total as u32 - cells - colors;
        if overlays > 0 {
            // Bound from the overlays' own offset rather than drawn with a
            // non-zero first instance, which WebGL2 has no native notion of.
            let offset = (cells + colors) as u64 * std::mem::size_of::<CellInstance>() as u64;
            pass.set_pipeline(&shared.compositor.overlays);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(offset..));
            pass.draw(0..4, 0..overlays);
        }
    }
}
