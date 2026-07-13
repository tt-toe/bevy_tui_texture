# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`bevy_tui_texture` is a Bevy plugin that renders ratatui terminal UIs as GPU textures using WGPU. It enables displaying terminal-style UIs on 2D sprites, 3D meshes, or UI elements with full GPU acceleration.

**Key Integration**: Bridges Bevy 0.19 + ratatui 0.30.2 + WGPU 29

## Build Commands

```bash
# Build the library
cargo build

# Build with release optimizations (LTO enabled, single codegen unit)
cargo build --release

# Run examples (development). ALWAYS use `cargo run --example`, not the bare
# binary in target/ — assets live in examples/assets/ (shared with the wasm
# build, see "WASM Demo Architecture" below) and are resolved via an
# `AssetPlugin { file_path: "examples/assets", .. }` override relative to
# CARGO_MANIFEST_DIR (== `cargo run`'s cwd); a bare binary looks next to the
# executable and e.g. shader_mesh renders black.
cargo run --example helloworld          # minimal static terminal
cargo run --example widget_catalog_2d   # 2D UI, mouse hit-testing, CJK, glyphs
cargo run --example widget_catalog_3d   # 3D mesh terminal with interaction
cargo run --example world_terminal      # world-unit-sized in-game screen (TuiRequest::world_quad)
cargo run --example multiple_terminals  # several terminals + Tab focus cycling
cargo run --example shader_mesh         # ExtendedMaterial CRT shader effects
cargo run --example retro_crt           # full CRT demo: glTF + ExtendedMaterial shader,
                                        #   additive reflection, overlay UI, camera modes
cargo run --example resize              # Tui::request_resize following the window size live
cargo run --example transparent_world_quad  # HUD-style see-through screen (transparent_reset_bg + AlphaMode::Blend)
cargo run --example benchmark           # full-frame rendering throughput (every cell redrawn each frame)
cargo run --example benchmark_partial   # BENCH_MODE=static|partial - isolates the cost of unchanged-frame
                                        #   and partial-row redraws (see the module doc comment)

# Run tests (no dedicated test directory - uses inline #[cfg(test)] modules
# next to the code they cover). Almost all are pure-CPU (HitRegions,
# pixel_to_cell/uv_to_cell coordinate mapping, Fonts::font_for_cell
# fallback order, staging-row padding math, resize, attach-churn, ...) and
# need no GPU at all; the one GPU-backed test
# (flush_renders_drawn_content_synchronously) skips gracefully if no
# adapter is available.
cargo test

# Same checks CI runs (.github/workflows/ci.yml):
cargo clippy --all-features --all-targets -- -D warnings
cargo doc --no-deps --all-features
cargo check --lib --no-default-features
cargo check --lib --no-default-features --features 2d
cargo check --lib --no-default-features --features 3d

# Examples log at RUST_LOG level via the dev-dependency bevy "bevy_log"
# feature (the lib's own bevy is default-features=false and logs nothing).

# WASM demo (browser-ready site into examples/web/; see "WASM Demo Architecture")
cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir examples/web \
  target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm
wasm-opt -Oz --strip-debug --strip-producers \
  --enable-nontrapping-float-to-int --enable-bulk-memory --enable-sign-ext \
  --enable-mutable-globals --enable-simd --enable-reference-types \
  -o examples/web/wasm_demo_bg.wasm examples/web/wasm_demo_bg.wasm
```

## Feature Flags

- `2d` (default) - 2D UI terminals (`TuiUi`, `TuiKind::Ui`)
- `3d` (default) - 3D mesh terminals (`TuiKind::WorldQuad`, `AttachTerminal`, mesh raycasting)
- `keyboard_input` (default) - Keyboard event handling
- `mouse_input` (default) - Mouse event handling for 2D UI and 3D mesh terminals
- `bold_italic_fonts` (opt-in) - Real bold/italic font slots (`Fonts::add_bold_fonts`/`add_italic_fonts`/`add_bold_italic_fonts`); without it, bold/italic are faked from the regular font
- `emoji` (opt-in, WIP) - Emoji-aware glyph handling (pulls in `unicode-properties`)

`TuiKind`'s variants gate individually (`Ui` needs `2d`, `WorldQuad` needs
`3d`, `Headless` is always available). Build with only one display surface,
e.g.:
```bash
cargo build --no-default-features --features "3d,keyboard_input,mouse_input"
```

## WASM Demo Architecture

`examples/wasm_demo.rs` is a thin wasm-bindgen shim around the full retro
CRT scene (glTF model + `ExtendedMaterial` CRT shader + additive
reflection + overlay UI + camera modes), packaged as a static site in
`examples/web/`.

**Assets are shared between native and wasm** via a single
`examples/assets/` directory (models, shaders, fonts) - there is no
per-target copy to keep in sync. `examples/retro_crt.rs`'s `build_app`
configures bevy's `AssetPlugin { file_path, .. }` per target: native reads
`examples/assets` (relative to `cargo run`'s cwd == `CARGO_MANIFEST_DIR`);
wasm fetches assets over HTTP relative to the *hosting page's URL*, via a
`file_path` value `build_app` takes as a parameter (cfg-gated to wasm32
only) rather than hard-coding - `wasm_demo.rs` supplies `"../assets"`,
since `examples/web/index.html` is served one directory below
`examples/assets/` (see "Local preview" below for the httpd root this
requires). Compile-time-embedded fonts (`include_bytes!`) are unaffected
by this - they reference `examples/assets/fonts/...` directly and need no
`AssetPlugin` involvement.

**The scene itself lives only in `examples/retro_crt.rs`** -
`wasm_demo.rs` pulls it in with `#[path = "retro_crt.rs"] mod retro_crt;`
and calls `retro_crt::build_app(..)` (marked `pub(crate)` for exactly this
reason) instead of duplicating the source. `retro_crt.rs`'s few
wasm32/WebGL2-only branches live inline behind
`#[cfg(target_arch = "wasm32")]` in that one file - there is nothing left
to keep in sync by hand:
- the `Window { canvas: Some("#bevy"), fit_canvas_to_parent: true, .. }`
  fields (harmless no-ops on native, so unconditional),
- OIT (`OrderIndependentTransparencySettings`) cfg-gated OFF on wasm -
  it needs storage buffers, which WebGL2 does not have,
- `Tonemapping::KhronosPbrNeutral` set explicitly on the camera on wasm
  (instead of `Camera3d`'s default `TonyMcMapface`) - see "Binary size"
  below.

`wasm_demo.rs` itself only adds what's specific to being loaded as a
browser module: the `#[wasm_bindgen(start)]` entry + `console_error_panic_hook`,
the Rust-side WebGL2 probe (see "Common Gotchas" below), the `"../assets"`
`AssetPlugin` override described above, and the `boot_status` module —
staged loading-overlay milestones sent to `window.__demoStatus` in
examples/web/index.html, which keeps the overlay up past wasm-bindgen
`init()` (that only means the event loop got registered) until the CRT
screen mesh is actually drawable. "Drawable" is detected via a
render-world system counting pending `PipelineCache` compiles into a shared
atomic — a merely-inserted material does NOT mean its mesh renders (bevy
skips meshes whose specialized pipeline is still compiling / whose WGSL is
still loading over HTTP).

**Build pipeline** (no trunk; plain cargo + wasm-bindgen CLI, whose
version must exactly match `Cargo.lock`'s `wasm-bindgen`, then `wasm-opt`
from binaryen for size reduction):
```bash
cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir examples/web \
  target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm
wasm-opt -Oz --strip-debug --strip-producers \
  --enable-nontrapping-float-to-int --enable-bulk-memory --enable-sign-ext \
  --enable-mutable-globals --enable-simd --enable-reference-types \
  -o examples/web/wasm_demo_bg.wasm examples/web/wasm_demo_bg.wasm
```
Generated (gitignored, `*.wasm` + `wasm_demo.js`): `examples/web/wasm_demo.js`
+ `examples/web/wasm_demo_bg.wasm`. Hand-maintained and committed:
`examples/web/index.html`, `examples/web/README.md` (deploy + local preview
instructions), `examples/web/.nojekyll`. No asset-copying step -
`examples/assets/` is fetched directly from its sibling location at
runtime, so there is nothing to duplicate or fall out of sync.

**Local preview**: `examples/web/index.html` fetches assets from the
sibling `../assets/`, so the httpd **must be rooted at `examples/`**, not
`examples/web/` itself (`python3 -m http.server 8080` from inside
`examples/`, then open `http://127.0.0.1:8080/web/`); `file://` does not
work for module scripts/wasm fetch either way. Details in
`examples/web/README.md`.

**Loading UI** (`examples/web/index.html`): the `.wasm` is tens of MB even
after `wasm-opt`, so the loading screen shows a real progress bar, not just
a static "loading…" label that looks stuck. It fetches
`wasm_demo_bg.wasm` itself via a wrapped `ReadableStream` that reports
bytes-read as they arrive, re-packages them into a fresh `Response` (so
`WebAssembly.instantiateStreaming` still gets used - no need to buffer the
whole file first), and passes that `Response` into wasm-bindgen's `init()`.
Falls back to an indeterminate sliding-stripe animation if the server
doesn't send `Content-Length`.

**Overlay panel collapse**: clicking the overlay's title bar (registered
as `Hit::PanelTitleBar` in `retro_crt.rs`, spanning the full top border
row in both states) toggles `AppState::panel_collapsed`, which
`render_overlay_terminal` acts on via `Tui::request_resize(cols, 1)` -
folding the panel down to just its title bar. No manual Node-size
bookkeeping needed: `request_resize` recreates the destination `Image` in
place at the new pixel size, and bevy_ui's image-content-size system
re-measures the `ImageNode` from the (now smaller) `Image` every frame, so
the on-screen Node shrinks/grows to match automatically.

**Binary size** (~31MB raw cargo output -> ~24MB final, see
`examples/web/README.md`'s "Binary size" section for the full breakdown): the
`wasm-opt -Oz` pass above accounts for most of the reduction (~15%); the
rest comes from `examples/wasm_demo.rs`'s wasm-only `bevy` dev-dependency
(Cargo.toml's `[target.'cfg(target_arch = "wasm32")'.dev-dependencies]`
block) dropping the `tonemapping_luts`/`zstd_rust` features (a ~680KB
embedded LUT + ktx2/zstd decoder, needed by `Camera3d`'s default
tonemapper but not by `Tonemapping::KhronosPbrNeutral`) and the native-only
`x11` feature. `#![no_std]` is not achievable here - Bevy and this crate's
own rendering/font stack (`raqote`, `rustybuzz`, `ratatui`) depend on
`std` throughout; there is no build-flag path to `no_std` short of forking
those crates.

## Core Architecture

### Abstraction Ladder

1. **`TerminalTexture` + `Tui::from_texture_state`** (src/setup.rs) - manual
   entity spawning, maximum flexibility (see `examples/tui_component.rs`,
   `examples/shader_mesh.rs`). User manages: entity spawning, input
   components, materials.

2. **`TuiRequest`** (src/setup.rs) - declarative spawning: spawn the
   request component (plus any `Node`/`Transform`/markers), the plugin's
   `materialize_tui_requests` system creates the texture and inserts the
   terminal components next frame - **no render resources in user code**.
   `TuiKind::Ui` (feature `2d`), `TuiKind::WorldQuad { height }` (feature
   `3d`, world-unit-sized quad, width follows texture aspect ratio),
   `TuiKind::Headless` (a `Tui` with no surface of its own). Fonts come as
   `Arc<Fonts>` (`TuiFontSource::Ready`, via `Into`) or through the
   AssetServer (`TuiFontSource::Asset` - the Wasm-safe path; the request
   stays pending until the `.ttf` loads).

3. **`AttachTerminal` + `AttachMaterial`** (src/setup.rs, feature `3d`) -
   attach a `Tui` to an *existing* mesh (e.g. a glTF primitive) instead of
   spawning one; `attach_terminal_system` re-claims the material every frame
   until a loader-driven overwrite (e.g. async glTF) stops recurring.
   Combine with a `TuiKind::Headless` request for the `Tui` entity itself.

All three levels share the same per-frame path: `Tui::draw()` (draws into
ratatui's buffer, marks dirty) + the plugin's `gpu_flush_system` (pure CPU,
extracts a draw payload for the render world to render - zero
render-resource parameters needed in user draw systems, no material
touching anywhere).

### Module Organization

**Backend Layer** (src/backend/):
- `bevy_backend.rs` - Main ratatui backend implementation (`BevyTerminalBackend`)
- `rasterize.rs` - Glyph rasterization using rustybuzz + raqote
- `programmatic_glyphs/` - Box-drawing, braille, block elements, powerline (procedurally generated)
- Two-pass rendering: background quads → foreground glyphs with alpha blending

**Plugin Layer** (src/bevy_plugin.rs):
- `TerminalPlugin` - Main Bevy plugin with input configuration
- `TerminalSystemSet` - Execution order: Input → UserUpdate → Render
- `gpu_flush_system` - plugin-owned per-`Tui` draw-payload extraction (pure
  CPU, main world); also applies any pending `Tui::request_resize` before
  flushing (recreates the destination `Image` in place, syncs the sibling
  `TerminalDimensions` component); `extract_tui_draws` + `render_tui_textures`
  (render world) do the actual GPU render, directly into the destination
  `GpuImage`; `resize_world_quad_meshes` (feature `3d`, after
  `gpu_flush_system`, keyed on `Changed<TerminalDimensions>`) recomputes a
  `TuiKind::WorldQuad` mesh's aspect ratio after a resize;
  `attach_terminal_system` (src/setup.rs, feature `3d`) - re-claims
  materials for `AttachTerminal`
- Components: `TerminalDimensions`, `TuiSurface`, `WorldQuadHeight` (feature `3d`)

**Input System** (src/input/):
- Event-driven architecture using Bevy messages (`TerminalEvent`)
- Focus management with Tab-key cycling
- Unified mouse handling: auto-detects 2D UI vs 3D mesh via raycasting
  (src/input/ray.rs, feature `3d`) when both `2d` and `3d` are enabled;
  single-purpose `mouse_input_system` variants otherwise

**Font System** (src/fonts.rs):
- TrueType font loading with rustybuzz
- Unicode support including CJK characters
- Font metrics: `min_width_px()`, `height_px()` for texture sizing

**Utilities** (src/utils/):
- `text_atlas.rs` - GPU texture cache (2048x2048px, square, WebGL2-safe max) for rendered glyphs
- `plan_cache.rs` - Text shaping cache for performance

### Critical Rendering Flow

1. User calls `tui.draw(|frame| { ... })` (or `draw_with_hits`) - draws into
   ratatui's buffer only, marks the `Tui` dirty *only if the diff actually
   changed a cell*. No GPU work, cheap every frame either way.
2. If dirty: the plugin's `gpu_flush_system` (main world, pure CPU) extracts
   a `TerminalDrawPayload` (pending glyph rasterizations + vertex/index
   data) from the backend.
3. A render-world system (`extract_tui_draws` + `render_tui_textures` in
   `bevy_plugin.rs`) renders that payload (two-pass: backgrounds → glyphs,
   `TerminalGpuState::render` in `backend/mod.rs`) directly into the
   destination `Image`'s own `GpuImage::texture_view` - the same texture
   any material's bind group already references. No asset mutation, no
   material touching, for any material type.

One-frame latency: the draw payload extracted this frame renders on the
next render pass. There is no CPU readback anywhere in this path (see
`Tui::read_back_blocking` for the explicit opt-in exception).

## Key Implementation Patterns

### Terminal Setup Pattern

```rust
// Setup system signature - no render resources at all
fn setup(mut commands: Commands) {
    // Font loading - from embedded bytes/file (or TuiFontSource::Asset)
    let font = Font::new(font_bytes)?;
    let fonts = Arc::new(Fonts::new(font, font_size));

    // Declarative terminal - materializes next frame
    commands.spawn((TuiRequest::ui(cols, rows, fonts), Node::default()));
}
```

### System Ordering

All terminal systems MUST use `TerminalSystemSet` for proper execution order:

```rust
.add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
.add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
```

Order: `Input` → `UserUpdate` → `Render`

### Input Event Handling

Input uses Bevy's message system (v0.18+), not legacy events:

```rust
fn handle_events(mut events: MessageReader<TerminalEvent>) {
    for event in events.read() {
        match event.event {
            TerminalEventType::KeyPress { key, modifiers } => { ... }
            TerminalEventType::MousePress { button, position } => { ... }
            _ => {}
        }
    }
}
```

### Terminal Rendering: Render World Only, No Material Touch

The main world does **zero** GPU work - rendering happens entirely in the
render world, directly into the destination `Image`'s own `GpuImage`
texture:

1. `Tui::draw()` renders into ratatui's buffer only (no GPU work), and
   marks the terminal dirty *only if the diff actually changed a cell*
   (a byte-identical redraw is a free no-op - ratatui itself always calls
   `Backend::flush()`, but `BevyTerminalBackend::flush()` early-returns
   before any shaping/vertex work when `cells_changed_last_draw` is
   false, so the "free" part holds all the way down, not just at the
   GPU-dirty gate).
2. The plugin's `gpu_flush_system` (main world, pure CPU) extracts a
   `TerminalDrawPayload` (glyph rasterizations + vertex/index data) from
   dirty terminals via `Tui::flush`/`BevyTerminalBackend::take_draw_payload`.
3. A render-world system (`extract_tui_draws` + `render_tui_textures` in
   `bevy_plugin.rs`) picks up each payload and renders it (background pass,
   foreground pass, `TerminalGpuState::render` in `backend/mod.rs`) directly
   into the destination `Image`'s `GpuImage::texture_view` - the very
   texture any material's bind group already references. No asset mutation
   happens, so there is nothing for Bevy's change detection to miss and
   nothing to re-touch, for `StandardMaterial`, `ExtendedMaterial`, or any
   other fully custom `Material` impl.
4. Render-world GPU state is split across two stores, each with its own
   key and eviction rule:
   - `TerminalGpuStore` - per-terminal state (screen-size uniform, index
     buffer, vertex buffers), keyed by destination `Image` asset id.
     Created lazily on first render, evicted automatically once that
     `GpuImage` disappears (the `Tui` despawned and its last
     `Handle<Image>` dropped) - no entity-level bookkeeping needed.
   - `SharedFontGpuStore` - the glyph atlas texture and background/foreground
     compositor pipelines, keyed by `Fonts::identity()` rather than by
     destination image, so terminals that share a `Fonts` share one glyph
     atlas and rasterize each glyph only once. Evicted once no live `Tui`
     reports that font key anymore (tracked via a `LiveFontKeys` liveness
     set, since a shared font's last user despawning leaves no destination
     image to key eviction off of).

```rust
fn render_terminal(mut screens: Query<&mut Tui, With<MyTerminal>>) {
    let Ok(mut term) = screens.single_mut() else { return };
    term.draw(|frame| { /* ... */ }); // zero render-resource parameters
}
```

Both 2D (`ImageNode`) and 3D (any material) terminals update automatically
through this same path - no per-material-type plugin registration needed.

## Font Requirements

- Fonts must be TrueType (.ttf) format
- Default example font: `assets/fonts/Mplus1Code-Regular.ttf`
- Font must support ASCII + any Unicode characters used
- Programmatic glyphs (box-drawing, braille) are procedurally generated if enabled
- Loading: `Font::new(&'static [u8])` for embedded data (`include_bytes!`),
  `Font::from_vec(Vec<u8>)` for runtime-loaded data (Arc-backed, no leaking)

## Common Gotchas

1. **Terminal creation needs no `RenderDevice`/`RenderQueue`** - `TerminalTexture::create` is pure CPU (`cols, rows, fonts, programmatic_glyphs, &mut Assets<Image>`); the GPU pipelines are built lazily in the render world on first render
2. **One-frame latency GPU updates** - `gpu_flush_system` (main world) extracts a draw payload the same frame a terminal is dirty; the render world renders it next render pass. There is no CPU readback anywhere in the hot path (see "Terminal Rendering: Render World Only, No Material Touch" above) - the only blocking readback is the explicit opt-in `Tui::read_back_blocking` (screenshots/tests, never call it every frame; goes through a request/response channel to the render world, so call it from a different thread than the one driving `App::update()` unless using `PipelinedRenderingPlugin`)
3. **Material updates need no touching at all** - not for `StandardMaterial`, not for custom materials. See "Terminal Rendering: Render World Only, No Material Touch" above
4. **System ordering** - Always use `TerminalSystemSet` or rendering may occur before input
5. **Font loading errors** - Verify font file exists and is valid TrueType format
6. **API signatures take `&mut Assets<...>`** (not `&mut ResMut<...>`) - callers with `ResMut` pass `&mut resmut` and deref coercion handles it; exclusive systems with direct `Assets` access work too
7. **Multi-camera mouse picking** - `mouse_input_system` builds rays per active camera via `Camera::viewport_to_world`, prioritized by descending `Camera::order` then hit distance (works with overlay-camera setups and any `ScalingMode`)
8. **Attaching a terminal to a glTF mesh** (see examples/retro_crt.rs; the model `assets/models/retro_crt.glb` is a remix of CrazyDrPants' "Retro CRT Computer", CC 4.0 — credit shown in the overlay):
   - glTF loading is async — a material inserted when the node's `Name` appears gets OVERWRITTEN by the loader's material later. Keep re-claiming (query entities that still carry the loader's `MeshMaterial3d<StandardMaterial>` each frame) until yours sticks. Once claimed, `attach_terminal_system` tracks the installed handle in a `TuiAttached` bookkeeping component and skips the remove+insert entirely on frames where nothing has changed — no per-frame archetype churn once settled, even for `AttachMaterial::standard()` targets that keep matching the query forever.
   - **Mesh-primitive entities are named `<MESH name>.<MATERIAL name>`** (e.g. `Object_2.Monitor_Glass`), NOT after the glTF node. Node names sit on parent entities without `MeshMaterial3d`. Target the primitive name exactly, or match the node name and walk descendants. Prefix matching is dangerous (`Object_2` also hits `Object_20`).
   - Model screens may sample a sub-rectangle of a texture atlas and/or be rotated; verify with the four-quadrant calibration pattern (`CRT_CALIBRATE=1`). To fix orientation prefer rewriting the mesh's `TEXCOORD_0` over `uv_transform`: the input hit-test uses raw mesh UVs, so `uv_transform` desyncs display from mouse picking. (The shipped retro_crt.glb has full-range upright UVs — no correction needed.)
   - Spawning glTF scenes in 0.19 (`WorldAssetRoot`; `Gltf::scenes` is `Vec<Handle<WorldAsset>>`) requires bevy features `bevy_world_serialization` + `reflect_auto_register` (panics on unregistered types without the latter) + image formats (`png`, `jpeg`).
9. **Custom `Material` with vertex colors** (see `BlurMaterial` in examples/retro_crt.rs):
   - Declaring `@location(1) color` in WGSL is NOT enough — without an explicit vertex layout in `Material::specialize` (`layout.0.get_layout(&[ATTRIBUTE_POSITION.at_shader_location(0), ATTRIBUTE_COLOR.at_shader_location(1), ...])` assigned to `descriptor.vertex.buffers`), bevy's default attribute order feeds NORMAL into location 1 and vertex colors silently have no effect.
   - A custom vertex layout breaks bevy's prepass/shadow pipelines: enabling shadows then dies with `prepass_pipeline` validation errors (0.19 quits the app on render errors). Insert `bevy::light::NotShadowCaster` + `NotShadowReceiver` on such meshes.
   - For additive blending (`BlendFactor::One/One`), modulate ALL light terms by the vertex color before output — any constant term added outside the multiply visibly ignores the vertex-color fade.
   - **Check the model for authored `COLOR_0` before generating vertex colors at runtime** — the glTF loader imports it as `ATTRIBUTE_COLOR` automatically. The `_0` reflection mesh ships a diamond fade (corners black, edge midpoints white). Design note: this reflection is unlit additive glow, so a fully custom material with explicit One/One blending is appropriate; use `ExtendedMaterial` + emissive-style addition instead when PBR lighting must be preserved.
10. **A wasm build MUST enable bevy's `bevy_winit` feature explicitly** (see the comment on the wasm32 dev-dependencies in Cargo.toml). On native it rides in via `x11`; trimming a wasm feature list "because x11 is native-only" silently drops the event-loop runner itself. Without WinitPlugin, `App::run()`'s fallback runner busy-spins `while plugins_state() == Adding {}` on the browser main thread — the `spawn_local`'d renderer-init future can then never run, so the tab hard-hangs with ZERO console output right after the wasm loads ("This page is slowing down Firefox", eventually "Script terminated by timeout" with the stack topping out in `bevy_app::App::plugins_state`); the hang looks exactly like a GPU/driver or upstream-Bevy failure but isn't. Related trap while debugging it: `.cargo/config.toml` sets `rustflags = ["-C","strip=symbols"]` for wasm32, so hang stacks are unreadable `wasm-function[N]` by default — and CLI `--config target....rustflags=[]` can NOT cancel it (cargo merges rustflags arrays); prefix `RUSTFLAGS=""` (env replaces config rustflags) plus `--config 'profile.wasm-release.strip="none"'` to get named stacks.

## GPU Texture Architecture

- **Glyph Atlas**: 2048x2048px RGBA8 texture, square and pinned to the WebGL2-safe `max_texture_dimension_2d` ceiling (see `CACHE_WIDTH`, `CACHE_HEIGHT` in src/backend/mod.rs) - same on native and wasm, so this surface never has a wasm-only failure mode. Shared across every terminal using the same `Fonts` (keyed by `Fonts::identity()` in the render world's `SharedFontGpuStore`), so terminals with a shared font rasterize and upload each glyph only once instead of duplicating the atlas per terminal
- **Terminal Textures**: Size = `cols * char_width_px` × `rows * char_height_px` (this one's aspect ratio is dictated by the caller's grid, not squared - unlike the atlas it isn't just a cache, so distorting it would distort the rendered content); callers requesting a very large grid/font combination are responsible for staying under whatever `max_texture_dimension_2d` their target GPU/backend reports
- **Render Pipelines**: Separate WGSL shaders for backgrounds (`composite_bg.wgsl`) and foreground text (`composite_fg.wgsl`)
- **Format**: Always `TextureFormat::Rgba8Unorm`

## Performance Considerations

- `BevyTerminalBackend::flush()` early-returns when a draw changed no cells
  (byte-identical redraws are free), and re-shapes only the rows
  `dirty_rows` marks changed, reusing a per-row vertex cache
  (`RowGeometry`) otherwise
- Glyph cache eviction uses LRU policy (via the `evictor` crate); the atlas
  itself is shared across every terminal using the same `Fonts` (see "GPU
  Texture Architecture" above), so a shared font's glyphs are rasterized
  and uploaded once, not once per terminal
- Per-terminal vertex/index GPU buffers are persistent and grow-only
  (`queue.write_buffer`d in place instead of recreated every dirty frame)
- Every dirty terminal's draw is recorded into one shared `CommandEncoder`
  and submitted once per frame, instead of one submit per terminal
- Release builds use `lto = "fat"` and `codegen-units = 1` for maximum optimization

## Version Compatibility

| bevy  | ratatui | wgpu | bevy_tui_texture |
|-------|---------|------|------------------|
| 0.19  | 0.30.2  | 29   | 0.3              |

Rust edition: 2024, `rust-version = "1.96"` (tracks bevy 0.19's own MSRV, not this crate's own code - see README.md's "MSRV policy")
