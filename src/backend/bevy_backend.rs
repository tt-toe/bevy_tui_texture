use std::collections::HashSet;
use std::sync::Arc;
use web_time::{Duration, Instant};

use crate::backend::rasterize::rasterize_glyph;
use crate::backend::TextBgVertexMember;
use crate::backend::TextCacheBgPipeline;
use crate::backend::TextCacheFgPipeline;
use crate::backend::TextVertexMember;
use crate::backend::Viewport;
use crate::backend::WgpuState;
use crate::colors::Rgb;
use crate::fonts::Fonts;
use crate::utils::plan_cache::PlanCache;
use crate::utils::text_atlas::Atlas;
use crate::utils::text_atlas::CacheRect;
use crate::utils::text_atlas::Key;
use crate::RandomState;
use bitvec::vec::BitVec;
use indexmap::IndexMap;
use ratatui::buffer::Cell;
use ratatui::text::Line;
use rustybuzz::ttf_parser::GlyphId;
use rustybuzz::UnicodeBuffer;
use wgpu::Buffer;
use wgpu::Device;
use wgpu::Queue;
use wgpu::Texture;
use wgpu::TextureView;

#[allow(dead_code)]
const NULL_CELL: Cell = Cell::new("");

#[allow(dead_code)]
pub(super) struct RenderInfo {
    cell: usize,
    cached: CacheRect,
    underline_pos_min: u16,
    underline_pos_max: u16,
}

/// Map from (x, y, glyph) -> (cell index, cache entry).
type Rendered = IndexMap<(i32, i32, GlyphId), RenderInfo, RandomState>;

/// Set of (x, y, glyph, char width).
type Sourced = HashSet<(i32, i32, GlyphId, u32), RandomState>;

/// A ratatui backend optimized for Bevy integration.
///
/// - No lifetime parameters
/// - Fonts are shared via Arc
/// - Device/Queue are not owned, passed as references during rendering
/// - No Surface management, renders directly to textures
/// - Simplified for Bevy's rendering pipeline
pub struct BevyTerminalBackend {
    // ====== Terminal dimensions ======
    pub(super) cols: u16,
    pub(super) rows: u16,

    // ====== Terminal state ======
    pub(super) cells: Vec<Cell>,
    pub(super) dirty_rows: Vec<bool>,
    pub(super) dirty_cells: BitVec,
    pub(super) cursor: (u16, u16),
    pub(super) viewport: Viewport,

    // ====== Rendering state ======
    pub(super) rendered: Vec<Rendered>,
    pub(super) sourced: Vec<Sourced>,

    // ====== Font management (Arc, no lifetime) ======
    pub(super) fonts: Arc<Fonts>,
    pub(super) plan_cache: PlanCache,
    pub(super) buffer: UnicodeBuffer,
    pub(super) row: String,
    pub(super) rowmap: Vec<u16>,

    // ====== Glyph cache (owned) ======
    pub(super) cached: Atlas,
    pub(super) text_cache: Texture,
    #[allow(dead_code)]
    pub(super) text_mask: Texture,

    // ====== Rendering pipelines (owned) ======
    pub(super) text_bg_compositor: TextCacheBgPipeline,
    pub(super) text_fg_compositor: TextCacheFgPipeline,
    pub(super) text_screen_size_buffer: Buffer,

    // ====== Draw data (owned) ======
    pub(super) bg_vertices: Vec<TextBgVertexMember>,
    pub(super) text_indices: Vec<[u32; 6]>,
    pub(super) text_vertices: Vec<TextVertexMember>,

    // ====== Pending GPU uploads ======
    pub(super) pending_cache_updates: Vec<(CacheRect, Vec<u32>, bool)>,

    // ====== wgpu state (owned) ======
    #[allow(dead_code)]
    pub(super) wgpu_state: WgpuState,

    // ====== Color settings ======
    pub(super) reset_fg: Rgb,
    pub(super) reset_bg: Rgb,

    // ====== Blink management (for future use) ======
    #[allow(dead_code)]
    pub(super) fast_blinking: BitVec,
    #[allow(dead_code)]
    pub(super) slow_blinking: BitVec,
    #[allow(dead_code)]
    pub(super) fast_duration: Duration,
    #[allow(dead_code)]
    pub(super) last_fast_toggle: Instant,
    #[allow(dead_code)]
    pub(super) show_fast: bool,
    #[allow(dead_code)]
    pub(super) slow_duration: Duration,
    #[allow(dead_code)]
    pub(super) last_slow_toggle: Instant,
    #[allow(dead_code)]
    pub(super) show_slow: bool,
}

/// Builder for BevyTerminalBackend. Fully synchronous, requires Device/Queue at build().
pub struct TerminalBuilder {
    fonts: Arc<Fonts>,
    cols: u16,
    rows: u16,
    reset_fg: Rgb,
    reset_bg: Rgb,
    viewport: Viewport,
    fast_blink: Duration,
    slow_blink: Duration,
}

impl TerminalBuilder {
    /// Create a new builder with the given fonts.
    pub fn new(fonts: Arc<Fonts>) -> Self {
        Self {
            fonts,
            cols: 80,
            rows: 24,
            reset_fg: [255, 255, 255], // WHITE
            reset_bg: [0, 0, 0],       // BLACK
            viewport: Viewport::Full,
            fast_blink: Duration::from_millis(200),
            slow_blink: Duration::from_millis(1000),
        }
    }

    /// Set terminal dimensions (columns, rows).
    pub fn with_dimensions(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }

    /// Set default foreground color.
    pub fn with_reset_fg(mut self, color: Rgb) -> Self {
        self.reset_fg = color;
        self
    }

    /// Set default background color.
    pub fn with_reset_bg(mut self, color: Rgb) -> Self {
        self.reset_bg = color;
        self
    }

    /// Set viewport mode.
    pub fn with_viewport(mut self, viewport: Viewport) -> Self {
        self.viewport = viewport;
        self
    }

    /// Build the BevyTerminalBackend.
    ///
    /// This is synchronous (unlike the original async Builder).
    /// Device and Queue are borrowed, not owned.
    pub fn build(self, device: &Device, _queue: &Queue) -> Result<BevyTerminalBackend, String> {
        use crate::backend::{
            build_text_bg_compositor, build_text_fg_compositor, build_wgpu_state, CACHE_HEIGHT,
            CACHE_WIDTH,
        };
        use std::mem::size_of;
        use wgpu::util::BufferInitDescriptor;
        use wgpu::util::DeviceExt;
        use wgpu::{
            AddressMode, BufferDescriptor, BufferUsages, Extent3d, FilterMode, SamplerDescriptor,
            TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
            TextureViewDescriptor,
        };

        // Calculate drawable dimensions
        let drawable_width = self.cols as u32 * self.fonts.min_width_px();
        let drawable_height = self.rows as u32 * self.fonts.height_px();

        // Create text cache texture (RGBA8, for colored glyphs)
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

        // Create text mask texture (R8, for alpha mask)
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

        // Create sampler
        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        // Create uniform buffers
        let text_screen_size_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Text Uniforms Buffer"),
            size: size_of::<[f32; 4]>() as u64,
            mapped_at_creation: false,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let atlas_size_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Atlas Size buffer"),
            contents: bytemuck::cast_slice(&[CACHE_WIDTH as f32, CACHE_HEIGHT as f32, 0.0, 0.0]),
            usage: BufferUsages::UNIFORM,
        });

        // For BevyTerminalBackend, we use a default texture format (Rgba8Unorm)
        // The actual render target format will be determined when render_to_texture is called
        let target_format = TextureFormat::Rgba8Unorm;

        // Build rendering pipelines
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

        // Build WgpuState
        let wgpu_state = build_wgpu_state(device, drawable_width, drawable_height);

        // Initialize Atlas
        let cached = Atlas::new(&self.fonts, CACHE_WIDTH, CACHE_HEIGHT);

        // Initialize plan cache
        let plan_cache = PlanCache::new(self.fonts.count().max(2));

        // Initialize blink timers
        let now = Instant::now();

        Ok(BevyTerminalBackend {
            cols: self.cols,
            rows: self.rows,
            cells: vec![],
            dirty_rows: vec![],
            dirty_cells: BitVec::new(),
            cursor: (0, 0),
            viewport: self.viewport,
            rendered: vec![],
            sourced: vec![],
            fonts: self.fonts,
            plan_cache,
            buffer: UnicodeBuffer::new(),
            row: String::new(),
            rowmap: vec![],
            cached,
            text_cache,
            text_mask,
            text_bg_compositor,
            text_fg_compositor,
            text_screen_size_buffer,
            bg_vertices: vec![],
            text_indices: vec![],
            text_vertices: vec![],
            pending_cache_updates: vec![],
            wgpu_state,
            reset_fg: self.reset_fg,
            reset_bg: self.reset_bg,
            fast_blinking: BitVec::new(),
            slow_blinking: BitVec::new(),
            fast_duration: self.fast_blink,
            last_fast_toggle: now,
            show_fast: true,
            slow_duration: self.slow_blink,
            last_slow_toggle: now,
            show_slow: true,
        })
    }
}

/// Convert tiny-skia Pixmap to Vec<u32> in RGBA8 format
fn pixmap_to_rgba8(pixmap: tiny_skia::Pixmap) -> Vec<u32> {
    pixmap
        .data()
        .chunks_exact(4)
        .map(|rgba| {
            // tiny-skia uses RGBA byte order
            let r = rgba[0];
            let g = rgba[1];
            let b = rgba[2];
            let a = rgba[3];
            // Pack into u32 (little-endian RGBA)
            u32::from_le_bytes([r, g, b, a])
        })
        .collect()
}

impl BevyTerminalBackend {
    /// Pre-populate programmatic glyphs into the texture atlas.
    ///
    /// This method renders all special glyphs (box-drawing, block elements, braille, powerline)
    /// using tiny-skia and queues them for GPU upload. Should be called once during initialization.
    ///
    /// # Arguments
    /// * `queue` - WGPU queue for uploading textures to GPU
    ///
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(String)` if any glyph fails to render
    pub fn populate_programmatic_glyphs(&mut self, queue: &Queue) -> Result<(), String> {
        use crate::backend::programmatic_glyphs::{
            all_programmatic_glyphs, render_programmatic_glyph,
        };
        use crate::utils::text_atlas::Key;
        use ratatui::style::Modifier;

        let width = self.fonts.min_width_px();
        let height = self.fonts.height_px();
        let font_id = self.fonts.last_resort_id();

        tracing::info!(
            "Pre-populating {} programmatic glyphs ({}x{} px)...",
            all_programmatic_glyphs().count(),
            width,
            height
        );

        let mut populated_count = 0;
        let mut skipped_count = 0;
        for unicode_char in all_programmatic_glyphs() {
            // Render glyph to bitmap using tiny-skia
            let pixmap = match render_programmatic_glyph(unicode_char, width, height) {
                Some(p) => p,
                None => {
                    // Skip glyphs not yet implemented
                    skipped_count += 1;
                    continue;
                }
            };

            // Convert tiny-skia Pixmap to Vec<u32> (RGBA8 format)
            let bitmap = pixmap_to_rgba8(pixmap);

            // Create atlas key
            let key = Key {
                style: Modifier::empty(),
                glyph: unicode_char as u32,
                font: font_id,
            };

            // Get atlas slot (this allocates space in the atlas)
            let rect = self.cached.get(&key, width, height);

            // Queue for GPU upload
            self.pending_cache_updates.push((*rect, bitmap, false));

            populated_count += 1;
        }

        // Flush all uploads to GPU immediately
        self.flush_cache_updates(queue);

        tracing::info!(
            "Successfully pre-populated {} programmatic glyphs ({} skipped - not yet implemented)",
            populated_count,
            skipped_count
        );
        Ok(())
    }

    /// Flush pending cache updates to GPU
    fn flush_cache_updates(&mut self, queue: &Queue) {
        use std::mem::size_of;
        use wgpu::{
            Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
        };

        for (cached, image, _is_emoji) in self.pending_cache_updates.drain(..) {
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
                bytemuck::cast_slice(&image),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(cached.width * size_of::<u32>() as u32),
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

    /// Render terminal content to an external GPU texture.
    ///
    /// This is the main rendering entry point for Bevy integration.
    ///
    /// # Rendering Pipeline
    ///
    /// 1. **GPU Uploads**: Uploads pending glyph rasterizations to `text_cache` texture
    /// 2. **Background Pass**: Renders colored backgrounds for each cell
    /// 3. **Foreground Pass**: Renders text glyphs sampled from glyph atlas
    ///
    /// # Arguments
    ///
    /// * `device` - Borrowed WGPU device (from Bevy's RenderDevice)
    /// * `queue` - Borrowed WGPU queue (from Bevy's RenderQueue)
    /// * `target` - Target texture view to render into (Bevy-managed texture)
    ///
    /// # Prerequisites
    ///
    /// **IMPORTANT**: `flush()` must be called before this method to prepare vertex data.
    pub fn render_to_texture(&mut self, device: &Device, queue: &Queue, target: &TextureView) {
        use ratatui::backend::Backend;
        use std::mem::size_of;
        use std::num::NonZeroU64;
        use wgpu::util::{BufferInitDescriptor, DeviceExt};
        use wgpu::{
            BufferUsages, CommandEncoderDescriptor, IndexFormat, LoadOp, Operations,
            RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
        };

        // Get terminal bounds (using Backend trait's size() method)
        let bounds = match Backend::size(self) {
            Ok(size) => size,
            Err(_) => return, // No content to render
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("Terminal Draw Encoder"),
        });

        // Upload pending glyph rasterizations to GPU textures
        for (cached, image, _is_emoji) in &self.pending_cache_updates {
            use std::mem::size_of;
            use wgpu::{
                Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
            };

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
                bytemuck::cast_slice(image),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(cached.width * size_of::<u32>() as u32),
                    rows_per_image: Some(cached.height),
                },
                Extent3d {
                    width: cached.width,
                    height: cached.height,
                    depth_or_array_layers: 1,
                },
            );

            // For mask texture (monochrome glyphs only, but we'll skip for now)
            // TODO: Implement mask texture upload if needed
        }

        if !self.text_vertices.is_empty() {
            // Update screen size uniform
            {
                let mut uniforms = queue
                    .write_buffer_with(
                        &self.text_screen_size_buffer,
                        0,
                        NonZeroU64::new(size_of::<[f32; 4]>() as u64).unwrap(),
                    )
                    .unwrap();
                uniforms.copy_from_slice(bytemuck::cast_slice(&[
                    bounds.width as f32 * self.fonts.min_width_px() as f32,
                    bounds.height as f32 * self.fonts.height_px() as f32,
                    0.0,
                    0.0,
                ]));
            }

            // Create vertex and index buffers
            let bg_vertices = device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Bg Vertices"),
                contents: bytemuck::cast_slice(&self.bg_vertices),
                usage: BufferUsages::VERTEX,
            });

            let fg_vertices = device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Vertices"),
                contents: bytemuck::cast_slice(&self.text_vertices),
                usage: BufferUsages::VERTEX,
            });

            let indices = device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Text Indices"),
                contents: bytemuck::cast_slice(&self.text_indices),
                usage: BufferUsages::INDEX,
            });

            {
                // Render pass: background + foreground
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

                // Background pass
                text_render_pass.set_pipeline(&self.text_bg_compositor.pipeline);
                text_render_pass.set_bind_group(0, &self.text_bg_compositor.fs_uniforms, &[]);
                text_render_pass.set_vertex_buffer(0, bg_vertices.slice(..));
                text_render_pass.draw_indexed(0..(self.bg_vertices.len() as u32 / 4) * 6, 0, 0..1);

                // Foreground pass
                text_render_pass.set_pipeline(&self.text_fg_compositor.pipeline);
                text_render_pass.set_bind_group(0, &self.text_fg_compositor.fs_uniforms, &[]);
                text_render_pass.set_bind_group(1, &self.text_fg_compositor.atlas_bindings, &[]);
                text_render_pass.set_vertex_buffer(0, fg_vertices.slice(..));
                text_render_pass.draw_indexed(
                    0..(self.text_vertices.len() as u32 / 4) * 6,
                    0,
                    0..1,
                );
            }
        } else {
            // If no text, just clear the target
            {
                let _clear_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("Terminal Clear Pass"),
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
            } // Render pass dropped automatically here
        }

        queue.submit(Some(encoder.finish()));
        // NOTE: No present() call - Bevy will handle that
    }

    /// Get the terminal dimensions in characters (columns, rows).
    /// Returns None if the backend is not properly initialized.
    pub fn dimensions(&self) -> Option<(u16, u16)> {
        if self.cols == 0 || self.rows == 0 {
            return None;
        }
        Some((self.cols, self.rows))
    }

    /// Get the text content of the terminal.
    pub fn get_text(&self) -> Vec<Line<'static>> {
        // TODO: Implement text extraction
        vec![]
    }

    /// Update fonts used by the backend.
    pub fn update_fonts(&mut self, new_fonts: Arc<Fonts>) {
        // Invalidate caches and mark all dirty
        self.dirty_rows.clear();
        self.cached.match_fonts(&new_fonts);
        self.fonts = new_fonts;
    }
}

// ====== Backend trait implementation ======

impl ratatui::backend::Backend for BevyTerminalBackend {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        use unicode_width::UnicodeWidthStr;

        let bounds = self.size()?;

        self.cells
            .resize(bounds.height as usize * bounds.width as usize, Cell::EMPTY);
        self.sourced.resize_with(
            bounds.height as usize * bounds.width as usize,
            Sourced::default,
        );
        self.rendered.resize_with(
            bounds.height as usize * bounds.width as usize,
            Rendered::default,
        );
        self.fast_blinking
            .resize(bounds.height as usize * bounds.width as usize, false);
        self.slow_blinking
            .resize(bounds.height as usize * bounds.width as usize, false);
        self.dirty_rows.resize(bounds.height as usize, true);

        for (x, y, cell) in content {
            let index = y as usize * bounds.width as usize + x as usize;

            self.fast_blinking.set(
                index,
                cell.modifier
                    .contains(ratatui::style::Modifier::RAPID_BLINK),
            );
            self.slow_blinking.set(
                index,
                cell.modifier.contains(ratatui::style::Modifier::SLOW_BLINK),
            );

            self.cells[index] = cell.clone();

            let width = cell.symbol().width().max(1);
            let start = (index + 1).min(self.cells.len());
            let end = (index + width).min(self.cells.len());
            self.cells[start..end].fill(NULL_CELL);
            self.dirty_rows[y as usize] = true;
        }

        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::layout::Position> {
        Ok(ratatui::layout::Position::new(self.cursor.0, self.cursor.1))
    }

    fn set_cursor_position<Pos: Into<ratatui::layout::Position>>(
        &mut self,
        position: Pos,
    ) -> std::io::Result<()> {
        let bounds = self.size()?;
        let pos: ratatui::layout::Position = position.into();
        self.cursor = (pos.x.min(bounds.width - 1), pos.y.min(bounds.height - 1));
        Ok(())
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.cells.clear();
        self.dirty_rows.clear();
        self.cursor = (0, 0);

        Ok(())
    }

    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        // Calculate drawable area based on cols/rows
        let pixel_width = self.cols as u32 * self.fonts.min_width_px();
        let pixel_height = self.rows as u32 * self.fonts.height_px();

        let width = pixel_width - inset_width;
        let height = pixel_height - inset_height;

        Ok(ratatui::layout::Size {
            width: (width / self.fonts.min_width_px()) as u16,
            height: (height / self.fonts.height_px()) as u16,
        })
    }

    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        let pixel_width = self.cols as u32 * self.fonts.min_width_px();
        let pixel_height = self.rows as u32 * self.fonts.height_px();

        let width = pixel_width - inset_width;
        let height = pixel_height - inset_height;

        Ok(ratatui::backend::WindowSize {
            columns_rows: ratatui::layout::Size {
                width: (width / self.fonts.min_width_px()) as u16,
                height: (height / self.fonts.height_px()) as u16,
            },
            pixels: ratatui::layout::Size {
                width: width as u16,
                height: height as u16,
            },
        })
    }

    /// Prepare vertex data for GPU rendering.
    ///
    /// This method performs the following operations:
    /// 1. Text shaping: Converts cell text into shaped glyphs using rustybuzz
    /// 2. Glyph caching: Rasterizes new glyphs and caches them in the GPU texture atlas
    /// 3. Vertex generation: Creates background and foreground vertices for rendering
    ///
    /// The generated vertices are stored in:
    /// - `bg_vertices`: Background quad vertices (one quad per glyph)
    /// - `text_vertices`: Foreground text vertices with UV coords into glyph atlas
    /// - `text_indices`: Index buffer for efficient quad rendering
    ///
    /// Pending glyph rasterizations are stored in `pending_cache_updates` and will be
    /// uploaded to the GPU texture in `render_to_texture()`.
    ///
    /// # Performance Notes
    ///
    /// Currently marks all cells as dirty for simplicity. Future optimization:
    /// track only changed cells based on `dirty_rows` from `draw()`.
    fn flush(&mut self) -> std::io::Result<()> {
        use crate::backend::c2c;
        use rustybuzz::shape_with_plan;
        use rustybuzz::ttf_parser::GlyphId;

        let bounds = self.size()?;

        // Clear buffers
        self.bg_vertices.clear();
        self.text_vertices.clear();
        self.text_indices.clear();
        self.pending_cache_updates.clear();

        // Mark all cells as dirty for now (TODO: optimize)
        self.dirty_cells.clear();
        self.dirty_cells.resize(self.cells.len(), true);

        // Process each row
        let mut index_offset = 0;
        for y in 0..bounds.height as usize {
            let row_start = y * bounds.width as usize;
            let row_end = (row_start + bounds.width as usize).min(self.cells.len());
            let row_cells = &self.cells[row_start..row_end];

            // Build row string for shaping
            self.row.clear();
            self.rowmap.clear();
            for (x, cell) in row_cells.iter().enumerate() {
                let symbol = cell.symbol();
                self.row.push_str(symbol);
                // Map each byte to its cell index
                for _ in 0..symbol.len() {
                    self.rowmap.push(x as u16);
                }
            }

            if self.row.is_empty() {
                continue;
            }

            // Shape the row
            let mut buffer = std::mem::take(&mut self.buffer);
            buffer.clear();
            for (idx, ch) in self.row.char_indices() {
                buffer.add(ch, idx as u32);
            }

            // For now, use font_for_cell on the first cell
            #[cfg(feature = "bold_italic_fonts")]
            let (font, _fake_bold, _fake_italic) = self.fonts.font_for_cell(&row_cells[0]);

            #[cfg(not(feature = "bold_italic_fonts"))]
            let (font, fake_bold, fake_italic) = {
                let (f, _, _) = self.fonts.font_for_cell(&row_cells[0]);
                (f, false, false) // Disable fake styling when feature is off
            };

            let glyph_buffer =
                shape_with_plan(font.font(), self.plan_cache.get(font, &mut buffer), buffer);

            let infos = glyph_buffer.glyph_infos();
            let positions = glyph_buffer.glyph_positions();

            // Process shaped glyphs
            let metrics = font.font();
            let advance_scale = self.fonts.height_px() as f32 / metrics.height() as f32;

            for (info, pos) in infos.iter().zip(positions.iter()) {
                let cluster = info.cluster as usize;
                if cluster >= self.rowmap.len() {
                    continue;
                }

                let cell_idx = self.rowmap[cluster] as usize;
                if cell_idx >= row_cells.len() {
                    continue;
                }

                let cell = &row_cells[cell_idx];
                let _glyph_id = GlyphId(info.glyph_id as u16);

                // Use per-cell font selection for proper styling
                #[cfg(feature = "bold_italic_fonts")]
                let (cell_font, cell_fake_bold, cell_fake_italic) = self.fonts.font_for_cell(cell);

                #[cfg(not(feature = "bold_italic_fonts"))]
                let (cell_font, cell_fake_bold, cell_fake_italic) = (font, fake_bold, fake_italic);

                // Calculate character width using unicode-width for precise glyph width
                use unicode_width::UnicodeWidthChar;
                let ch = cell.symbol().chars().next().unwrap_or(' ');
                let ch_width = ch.width().unwrap_or(1).max(1) as u32;
                let glyph_width_px = ch_width * self.fonts.min_width_px();

                // Check if this character is an emoji
                #[cfg(feature = "emoji_support")]
                fn is_emoji(ch: char) -> bool {
                    use unicode_properties::UnicodeEmoji;
                    // Simplify emoji detection - just check if it's an emoji character
                    ch.is_emoji_char()
                }

                #[cfg(not(feature = "emoji_support"))]
                fn is_emoji(_ch: char) -> bool {
                    false
                }

                let is_emoji = is_emoji(ch);

                // Check if this is a programmatic glyph that was pre-rendered
                use crate::backend::programmatic_glyphs::is_programmatic_glyph;
                let is_programmatic = is_programmatic_glyph(ch);

                // Create cache key
                // For programmatic glyphs: use Unicode codepoint + last_resort font (matches populate_programmatic_glyphs)
                // For font glyphs: use shaped glyph ID + actual font

                #[cfg(feature = "bold_italic_fonts")]
                let style = cell.modifier
                    & (ratatui::style::Modifier::BOLD | ratatui::style::Modifier::ITALIC);

                #[cfg(not(feature = "bold_italic_fonts"))]
                let style = ratatui::style::Modifier::empty();

                let key = if is_programmatic {
                    Key {
                        style,
                        glyph: ch as u32,
                        font: self.fonts.last_resort_id(),
                    }
                } else {
                    Key {
                        style,
                        glyph: info.glyph_id,
                        font: cell_font.id(),
                    }
                };

                let cached = self
                    .cached
                    .get(&key, glyph_width_px, self.fonts.height_px());

                // If not cached, render the glyph
                if !cached.cached() {
                    if is_programmatic {
                        // Render programmatic glyph on-demand if not pre-cached
                        use crate::backend::programmatic_glyphs::render_programmatic_glyph;

                        if let Some(pixmap) =
                            render_programmatic_glyph(ch, glyph_width_px, self.fonts.height_px())
                        {
                            let bitmap = pixmap_to_rgba8(pixmap);
                            self.pending_cache_updates.push((*cached, bitmap, false));
                        } else {
                            tracing::warn!(
                                "Failed to render programmatic glyph '{}' (U+{:04X})",
                                ch,
                                ch as u32
                            );
                        }
                    } else {
                        // Calculate glyph bearing offset to apply during rasterization
                        let bearing_offset_x = pos.x_offset as f32 * advance_scale;

                        // Don't apply fake styling to emoji characters to avoid distortion
                        let final_fake_italic = cell_fake_italic && !is_emoji;
                        let final_fake_bold = cell_fake_bold && !is_emoji;

                        let (rect, image) = rasterize_glyph(
                            cached,
                            metrics,
                            info,
                            final_fake_italic, // Don't distort emoji
                            final_fake_bold,   // Don't distort emoji
                            advance_scale,
                            glyph_width_px,   // Use actual glyph width
                            bearing_offset_x, // Apply offset in atlas
                        );

                        self.pending_cache_updates.push((rect, image, false));
                    }
                }

                // Calculate screen position - align to cell grid since offset is already in atlas
                let screen_x = cell_idx as f32 * self.fonts.min_width_px() as f32;
                let screen_y = y as f32 * self.fonts.height_px() as f32;

                // Get colors
                let reverse = cell.modifier.contains(ratatui::style::Modifier::REVERSED);
                let bg_color = if reverse {
                    c2c(cell.fg, self.reset_fg)
                } else {
                    c2c(cell.bg, self.reset_bg)
                };
                let fg_color = if reverse {
                    c2c(cell.bg, self.reset_bg)
                } else {
                    c2c(cell.fg, self.reset_fg)
                };

                let [r, g, b] = bg_color;
                let bg_color_u32 = u32::from_be_bytes([r, g, b, 255]);

                let [r, g, b] = fg_color;
                let fg_color_u32 = u32::from_be_bytes([r, g, b, 255]);

                // Generate indices
                self.text_indices.push([
                    index_offset,
                    index_offset + 1,
                    index_offset + 2,
                    index_offset + 2,
                    index_offset + 3,
                    index_offset + 1,
                ]);
                index_offset += 4;

                // Render at actual glyph width (no compression)
                let render_width_px = glyph_width_px as f32;

                // Background vertices
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x, screen_y],
                    bg_color: bg_color_u32,
                });
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x + render_width_px, screen_y],
                    bg_color: bg_color_u32,
                });
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x, screen_y + self.fonts.height_px() as f32],
                    bg_color: bg_color_u32,
                });
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [
                        screen_x + render_width_px,
                        screen_y + self.fonts.height_px() as f32,
                    ],
                    bg_color: bg_color_u32,
                });

                // Text vertices - 1:1 mapping between atlas and screen
                let uv_x = cached.x as f32;
                let uv_y = cached.y as f32;
                let uv_w = cached.width as f32; // Matches glyph_width_px
                let uv_h = cached.height as f32;

                self.text_vertices.push(TextVertexMember {
                    vertex: [screen_x, screen_y],
                    uv: [uv_x, uv_y],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
                self.text_vertices.push(TextVertexMember {
                    vertex: [screen_x + render_width_px, screen_y],
                    uv: [uv_x + uv_w, uv_y],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
                self.text_vertices.push(TextVertexMember {
                    vertex: [screen_x, screen_y + self.fonts.height_px() as f32],
                    uv: [uv_x, uv_y + uv_h],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
                self.text_vertices.push(TextVertexMember {
                    vertex: [
                        screen_x + render_width_px,
                        screen_y + self.fonts.height_px() as f32,
                    ],
                    uv: [uv_x + uv_w, uv_y + uv_h],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
            }

            // Restore buffer (clear GlyphBuffer back to UnicodeBuffer)
            self.buffer = glyph_buffer.clear();
        }

        Ok(())
    }

    fn clear_region(
        &mut self,
        _clear_type: ratatui::backend::ClearType,
    ) -> Result<(), Self::Error> {
        // For now, just delegate to clear() for all clear types
        self.clear()
    }
}
