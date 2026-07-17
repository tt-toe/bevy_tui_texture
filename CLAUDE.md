# CLAUDE.md

Guidance for Claude Code (claude.ai/code) when working in this repository.

## Project Overview

`bevy_tui_texture` renders ratatui terminal UIs as GPU textures inside a
Bevy app — on 2D UI nodes, 3D meshes, or existing glTF screens — with no
CPU readback anywhere in the hot path.

**Version pins**: bevy 0.19 + ratatui 0.30.2 + wgpu 29 (wgpu must exactly
match bevy's pin — bump together; both are re-exported so downstream code
can name matching types).

## Build & Verify

```bash
cargo build                       # library
cargo build --release             # LTO fat, codegen-units=1

# Examples: ALWAYS `cargo run --example`, never the bare target/ binary —
# assets live in examples/assets/ (shared with the wasm build) and resolve
# via an `AssetPlugin { file_path: "examples/assets", .. }` override
# relative to CARGO_MANIFEST_DIR; a bare binary looks next to the
# executable and e.g. shader_mesh renders black.
cargo run --example helloworld          # minimal static terminal
cargo run --example widget_catalog_2d   # 2D UI, mouse hit-testing, CJK, glyphs
cargo run --example widget_catalog_3d   # 3D mesh terminal with interaction
cargo run --example world_terminal      # world-unit screen (TuiRequest::world_quad)
cargo run --example multiple_terminals  # several terminals + Tab focus cycling
cargo run --example shader_mesh         # ExtendedMaterial CRT shader effects
cargo run --example retro_crt           # full CRT demo: glTF + shader + overlay UI
cargo run --example resize              # Tui::request_resize following the window
cargo run --example transparent_world_quad  # see-through HUD screen
cargo run --example benchmark           # full-frame throughput
cargo run --example benchmark_partial   # BENCH_MODE=static|partial redraw costs

# Tests: inline #[cfg(test)] modules next to the code (no tests/ dir).
# Pure CPU except one GPU-backed test that skips without an adapter.
cargo test

# What CI runs (.github/workflows/ci.yml):
cargo test --all-features
cargo clippy --all-features --all-targets -- -D warnings
cargo doc --no-deps --all-features
cargo check --lib --no-default-features            # + --features 2d / 3d variants
cargo check --target wasm32-unknown-unknown --example wasm_demo
```

Examples log at `RUST_LOG` level via the dev-dependency bevy's `bevy_log`
feature (the lib's own bevy is default-features=false and logs nothing).

## Feature Flags

- `2d` (default) — 2D UI terminals (`TuiUi`, `TuiKind::Ui`)
- `3d` (default) — 3D mesh terminals (`TuiKind::WorldQuad`, `AttachTerminal`, mesh raycasting)
- `keyboard_input` (default) — keyboard event handling
- `mouse_input` (default) — mouse events for 2D UI and 3D mesh terminals;
  touch rides the same path (touch position feeds `CursorPosition`, a tap
  emulates the left button)
- `bold_italic_fonts` (opt-in) — real bold/italic font slots
  (`Fonts::add_bold_fonts`/`add_italic_fonts`/`add_bold_italic_fonts`);
  otherwise bold/italic are faked from the regular font
- `emoji` (opt-in, WIP) — emoji-aware glyph handling (pulls in `unicode-properties`)
- `ascii_fast_shaping` (opt-in) — bypasses rustybuzz for rows that are all
  single ASCII printable bytes (see IMPROVEMENT.md A3). Assumes zero
  x_offset, which most monospace fonts satisfy but isn't guaranteed.
  Silently inert when `bold_italic_fonts` is enabled (that feature makes
  per-cell font selection meaningful; this path assumes one font per row)

`TuiKind` variants gate individually: `Ui` needs `2d`, `WorldQuad` needs
`3d`, `Headless` is always available. One-surface builds work:
`cargo build --no-default-features --features "3d,keyboard_input,mouse_input"`.

## Core Architecture

### Abstraction ladder (src/setup.rs)

1. **`TuiRequest`** (default choice) — declarative: spawn the component
   (plus any `Node`/`Transform`/markers), `materialize_tui_requests`
   creates the texture and inserts the terminal components next frame — no
   render resources in user code. `TuiKind::Ui` (2D), `TuiKind::WorldQuad
   { height }` (world-unit quad, width follows texture aspect),
   `TuiKind::Headless` (a `Tui` with no surface). Fonts arrive as
   `Arc<Fonts>` (`TuiFontSource::Ready`, via `Into`) or through the
   AssetServer (`TuiFontSource::Asset` — the Wasm-safe path; the request
   stays pending until the `.ttf` loads).
2. **`AttachTerminal` + `AttachMaterial`** (feature `3d`) — put a
   (typically headless) `Tui` on an *existing* mesh, e.g. a glTF
   primitive. `attach_terminal_system` re-claims the material every frame
   until the async loader stops overwriting it, then tracks the installed
   handle in `TuiAttached` and goes idle (no archetype churn once settled).
3. **`TerminalTexture::create` + `Tui::from_texture_state`** — manual
   escape hatch (`examples/tui_component.rs`, `examples/shader_mesh.rs`).

All three share the same per-frame path below.

### Rendering pipeline (the load-bearing part)

1. `Tui::draw(|frame| ...)` / `draw_with_hits` — pure CPU, fills ratatui's
   buffer, marks the `Tui` dirty **only if the diff changed a cell**.
   Byte-identical redraws are free all the way down: ratatui always calls
   `Backend::flush()`, but `BevyTerminalBackend::flush()` early-returns
   before any shaping/vertex work when nothing changed. Calling `draw`
   every frame is the intended pattern.
2. If dirty, the plugin's `gpu_flush_system` (main world, pure CPU)
   extracts a `TerminalDrawPayload` (pending glyph rasterizations +
   vertex/index data); it also applies any pending `Tui::request_resize`
   first (recreates the destination `Image` in place at the same handle,
   syncs `TerminalDimensions`; `resize_world_quad_meshes` then fixes a
   world quad's aspect on `Changed<TerminalDimensions>`).
3. Render world: `extract_tui_draws` + `render_tui_textures`
   (src/bevy_plugin.rs) render the payload — two passes, background quads
   then glyphs (`TerminalGpuState::render` in src/backend/mod.rs) —
   **directly into the destination `Image`'s own `GpuImage::texture_view`**,
   the same texture any material's bind group already references. No asset
   mutation, no material touching, for `StandardMaterial`,
   `ExtendedMaterial`, or any custom `Material`. Both 2D (`ImageNode`) and
   3D terminals update through this one path.

**Same-frame latency is structural**: `render_tui_textures` runs in the
`RenderGraph` schedule's `RenderGraphSystems::Begin` set, chained strictly
before `RenderGraphSystems::Render` (camera passes) and
`RenderGraphSystems::Submit` (one batched submit for terminal and camera
commands alike — `flush_tui_commands`, riding the shared `RenderContext`
encoder). A material samples this frame's content, not last frame's. The
only CPU readback is the explicit opt-in `Tui::read_back_blocking`.

### Render-world GPU state & eviction

- `TerminalGpuStore` — per-terminal state (screen-size uniform, persistent
  grow-only vertex/index buffers, `write_buffer`d in place), keyed by
  destination `Image` asset id; created lazily on first render, evicted
  when that `GpuImage` disappears.
- `SharedFontGpuStore` — glyph atlas + bg/fg compositor pipelines, keyed
  by `Fonts::identity()`: terminals sharing a `Fonts` share one atlas and
  rasterize each glyph once. Evicted via the `LiveFontKeys` liveness set
  once no live `Tui` reports that font key.

### Partial redraw

`flush()` re-shapes only the rows `dirty_rows` marks changed, reusing a
per-row vertex cache (`RowGeometry`). `take_draw_payload` emits either a
full payload (every row, `LoadOp::Clear`) — used whenever the destination
texture can't be trusted (`full_redraw_needed`: just created / resized /
cleared / fonts swapped / pending-payload overwrite) — or a partial one
(only dirty rows, each preceded by a synthesized row-clear quad,
`LoadOp::Load`). CPU concatenation and GPU upload both scale with dirty
rows. Glyph-atlas eviction is LRU (`evictor` crate).

### Other modules

- **src/backend/** — `bevy_backend.rs` (ratatui `Backend` impl),
  `rasterize.rs` (rustybuzz + raqote), `programmatic_glyphs/`
  (box-drawing, braille, block elements, powerline — procedural)
- **src/bevy_plugin.rs** — `TerminalPlugin`, `TerminalSystemSet`
  (Input → UserUpdate → Render in `Update`), the systems above
- **src/input/** — message-driven (`TerminalEvent` via `MessageReader`,
  bevy 0.18+ messages, not legacy events); focus management with Tab
  cycling; unified mouse handling that auto-detects 2D UI vs 3D mesh via
  raycasting (src/input/ray.rs) when both features are on; touch fallback
  (see Gotchas)
- **src/fonts.rs** — TrueType via rustybuzz; CJK; metrics
  `min_width_px()` / `height_px()` for texture sizing
- **src/utils/** — `text_atlas.rs` (glyph cache texture),
  `plan_cache.rs` (shaping cache)

## Key Patterns

```rust
// Setup: no render resources anywhere in the signature.
fn setup(mut commands: Commands) {
    let fonts = Arc::new(Fonts::new(Font::new(font_bytes)?, font_size));
    commands.spawn((TuiRequest::ui(cols, rows, fonts), Node::default()));
}

// Per-frame draw. `else return` is REQUIRED: TuiRequest materializes one
// frame after spawn.
fn render_terminal(mut screens: Query<&mut Tui, With<MyTerminal>>) {
    let Ok(mut term) = screens.single_mut() else { return };
    term.draw(|frame| { /* ratatui widgets */ });
}
```

User systems MUST be placed in `TerminalSystemSet` (`Input` →
`UserUpdate` → `Render`):

```rust
.add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
.add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
```

Input events:

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

Fonts: TrueType only. `Font::new(&'static [u8])` for `include_bytes!`,
`Font::from_vec(Vec<u8>)` for runtime-loaded data (Arc-backed, never
`Box::leak`). Default example font:
`examples/assets/fonts/Mplus1Code-Regular.ttf`.

## Common Gotchas

1. **Terminal creation needs no `RenderDevice`/`RenderQueue`** —
   `TerminalTexture::create` is pure CPU; GPU pipelines build lazily in
   the render world on first render.
2. **No material touching, ever** — see "Rendering pipeline" above. Also
   no per-material-type plugin registration.
3. **API signatures take `&mut Assets<...>`**, not `&mut ResMut<...>` —
   pass `&mut resmut` and deref coercion handles it.
4. **Multi-camera mouse picking** — rays are built per active camera via
   `Camera::viewport_to_world` (correct for any projection/`ScalingMode`),
   prioritized by descending `Camera::order`, then hit distance.
5. **Touch input** — winit never synthesizes mouse events from touches.
   `update_cursor_position_system` falls back to the first active touch's
   position (and, on the release frame, the just-released touch's last
   position), and `emit_button_events` treats a tap as a left-button
   press/release. Consumers reading `CursorPosition` instead of
   `Window::cursor_position()` get touch support for free (see
   `update_camera_rotation` in examples/retro_crt.rs).
6. **`Tui::read_back_blocking(&channel)`** (screenshots/tests only, never
   per frame) blocks on a request/response channel to the render world —
   call it from a different thread than the one driving `App::update()`
   unless `PipelinedRenderingPlugin` is active.
7. **Attaching to a glTF mesh** (examples/retro_crt.rs; the model
   `retro_crt.glb` is a remix of CrazyDrPants' "Retro CRT Computer",
   CC 4.0 — credit shown in the overlay):
   - glTF loading is async — a material inserted early gets OVERWRITTEN by
     the loader later. `attach_terminal_system` keeps re-claiming until it
     sticks (see "Abstraction ladder").
   - **Mesh-primitive entities are named `<MESH>.<MATERIAL>`** (e.g.
     `Object_2.Monitor_Glass`), NOT after the glTF node. Node names sit on
     parent entities without `MeshMaterial3d`. Exact-match the primitive
     name; prefix matching is dangerous (`Object_2` also hits `Object_20`).
   - Model screens may sample an atlas sub-rect and/or be rotated; verify
     with the calibration pattern (`CRT_CALIBRATE=1`). Prefer rewriting
     `TEXCOORD_0` over `uv_transform` — the hit-test uses raw mesh UVs, so
     `uv_transform` desyncs display from picking. (The shipped
     retro_crt.glb has full-range upright UVs.)
   - For a self-illuminating screen, route the terminal texture through
     the **emissive channel** (`emissive_texture` + white `emissive`,
     black `base_color`) — bevy_pbr adds emissive after the
     light-dependent terms, so content stays visible at any scene light
     level while metallic/roughness/reflectance still give a glass
     specular highlight (see `claim_object2_screen`).
   - Spawning glTF scenes in 0.19 (`WorldAssetRoot`; `Gltf::scenes` is
     `Vec<Handle<WorldAsset>>`) requires bevy features
     `bevy_world_serialization` + `reflect_auto_register` (panics without
     the latter) + image formats (`png`, `jpeg`).
8. **Custom `Material` with vertex colors** (`BlurMaterial` in
   examples/retro_crt.rs):
   - Declaring `@location(1) color` in WGSL is NOT enough — without an
     explicit vertex layout in `Material::specialize`, bevy feeds NORMAL
     into location 1 and vertex colors silently do nothing.
   - A custom vertex layout breaks the prepass/shadow pipelines (0.19
     quits the app on render errors) — insert `NotShadowCaster` +
     `NotShadowReceiver`.
   - For additive blending (`One/One`), modulate ALL light terms by the
     vertex color, or the constant term visibly ignores the fade.
   - Check the model for authored `COLOR_0` before generating vertex
     colors at runtime — the glTF loader imports it as `ATTRIBUTE_COLOR`
     (the `_0` reflection mesh ships a diamond fade).
9. **A wasm build MUST enable bevy's `bevy_winit` feature explicitly**
   (see the wasm32 dev-dependencies comment in Cargo.toml). On native it
   rides in via `x11`; dropping it on wasm silently drops the event-loop
   runner — `App::run()`'s fallback busy-spins on the browser main thread
   and the tab hard-hangs with ZERO console output (stack topping out in
   `bevy_app::App::plugins_state`). Looks exactly like a GPU/driver
   failure but isn't. Debugging trap: `.cargo/config.toml` strips wasm
   symbols, and CLI `--config target....rustflags=[]` can NOT cancel it
   (cargo merges arrays) — use `RUSTFLAGS=""` (env replaces config) plus
   `--config 'profile.wasm-release.strip="none"'` for named stacks.

## GPU Texture Facts

- **Glyph atlas**: 2048×2048 RGBA8 (`CACHE_WIDTH`/`CACHE_HEIGHT` in
  src/backend/mod.rs) — square and pinned to the WebGL2-guaranteed
  `max_texture_dimension_2d`, identical on native and wasm, so it has no
  wasm-only failure mode. Shared per `Fonts::identity()`; LRU-evicted.
- **Terminal textures**: `cols·char_w × rows·char_h`, `Rgba8Unorm`.
  Staying under the target GPU's `max_texture_dimension_2d` for huge
  grid/font combinations is the caller's responsibility.
- **Shaders**: `composite_bg.wgsl` (backgrounds) + `composite_fg.wgsl`
  (glyphs).

## WASM Demo Architecture

`examples/wasm_demo.rs` is a thin wasm-bindgen shim around the full retro
CRT scene, packaged as a static site in `examples/web/`.

**One scene, one asset tree.** The scene lives only in
`examples/retro_crt.rs`; `wasm_demo.rs` pulls it in via `#[path]` and
calls `retro_crt::build_app(asset_path)`. Assets are shared through the
single `examples/assets/` directory: native reads it relative to `cargo
run`'s cwd, wasm fetches over HTTP relative to the hosting page —
`wasm_demo.rs` passes `"../assets"` because `examples/web/index.html` is
served one directory below. Compile-time `include_bytes!` fonts bypass
`AssetPlugin` entirely. `retro_crt.rs`'s few wasm-only branches live
inline behind `#[cfg(target_arch = "wasm32")]`: the `Window { canvas:
Some("#bevy"), fit_canvas_to_parent: true, .. }` fields (no-ops on
native, so unconditional), OIT cfg'd OFF (needs storage buffers, absent
on WebGL2), and `Tonemapping::KhronosPbrNeutral` (see "Binary size").

**`wasm_demo.rs` adds only browser-module concerns**:
- `#[wasm_bindgen(start)]` entry; a panic hook that both logs to the
  console AND forwards `panicked (heap N MB): ...` to the loading overlay
  via `window.__demoStatus` — on mobile Safari the overlay is often the
  only visible console. (A true OOM abort — failed `memory.grow` — never
  runs the hook; it traps to a JS "Unreachable code" RuntimeError, which
  index.html annotates as probable OOM.)
- A WebGL2 probe on a throwaway canvas (probing `#bevy` would take its
  context and break wgpu's later `getContext`).
- `boot_status`: staged loading milestones sent to `window.__demoStatus`.
  The overlay stays up past wasm-bindgen `init()` until the CRT screen is
  actually drawable — detected by a render-world system counting pending
  `PipelineCache` compiles (an inserted material does NOT mean its mesh
  renders). Waiting stages emit a once-per-second heartbeat with elapsed
  seconds + heap MB; a failed `.glb` load is surfaced on the overlay
  instead of stalling forever.
- **Surface-size clamps** (`MAX_PHYSICAL_PX = 2032`, deliberately under
  the 2048 WebGL2 ceiling — Safari's winit path measures css×DPR with its
  own rounding and an exact clamp can tip past the cap, leaving the
  surface unconfigured and panicking on the next `get_current_texture`):
  1. `clamp_canvas_to_safe_texture_size` caps the canvas's CSS
     `max-width`/`max-height` at `MAX_PHYSICAL_PX / devicePixelRatio`
     (spoofing `window.devicePixelRatio` does NOT work — winit measures
     via `ResizeObserver`'s `devicePixelContentBoxSize`).
  2. `clamp_initial_window_resolution` also caps bevy's initial
     `WindowResolution` before `run()` — bevy configures the FIRST
     frame's surface from that value × real DPR before any
     ResizeObserver report, so on a DPR-3 phone 1024×768 becomes
     3072×2304 and fails; the clamped (briefly distorted) size lives
     only until `fit_canvas_to_parent` takes over.
- `suppress_canvas_escape_key`: a DOM `keydown` listener registered on
  the canvas BEFORE `App::run()` calls `stop_immediate_propagation()` so
  winit (which registers lazily at window creation) never sees Escape —
  same-target DOM listeners fire in registration order.

**`examples/web/index.html`** (hand-maintained, committed, alongside
`README.md` and `.nojekyll`; `wasm_demo.js` + `*.wasm` are generated and
gitignored): real download progress bar (wraps the `.wasm` fetch in a
counting `ReadableStream`, still feeds `instantiateStreaming`);
`touch-action: none` on the canvas so mobile Safari doesn't claim taps;
fatal-error latch (first panic/error wins — post-trap rethrows would
overwrite the root cause); `error`/`unhandledrejection`/
`webglcontextlost` listeners with an OOM explainer; a spinner+seconds
liveness ticker (distinguishes "main thread alive, async step stuck" from
"main thread stalled"); and mirroring of the first few + latest
`console.warn/error` lines into the overlay. Bump `ASSET_VERSION` on
every redeploy — the JS glue and wasm are a matched pair and stale caches
mixing versions fail with `LinkError`.

**Build pipeline** (no trunk; wasm-bindgen CLI version must exactly match
`Cargo.lock`'s `wasm-bindgen`):

```bash
cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir examples/web \
  target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm
wasm-opt -Oz --strip-debug --strip-producers \
  --enable-nontrapping-float-to-int --enable-bulk-memory --enable-sign-ext \
  --enable-mutable-globals --enable-simd --enable-reference-types \
  -o examples/web/wasm_demo_bg.wasm examples/web/wasm_demo_bg.wasm
```

**Local preview**: the page fetches `../assets/`, so the httpd must be
rooted at `examples/` (`python3 -m http.server 8080` from `examples/`,
open `http://127.0.0.1:8080/web/`); `file://` cannot work. Details in
`examples/web/README.md`.

**Binary size**: ~31MB raw → ~24MB after `wasm-opt -Oz` plus the wasm-only
bevy dev-dependency dropping `tonemapping_luts`/`zstd_rust` (a ~680KB LUT
+ decoder needed only by the default tonemapper — hence
`KhronosPbrNeutral`) and native-only `x11`. `#![no_std]` is not
achievable (bevy, raqote, rustybuzz, ratatui need `std`). Full breakdown
in `examples/web/README.md`.

**Overlay panel collapse**: clicking the overlay's title bar
(`Hit::PanelTitleBar`) toggles a `Tui::request_resize(cols, 1)` — the
`Image` is recreated in place and bevy_ui re-measures the `ImageNode`
automatically, no manual Node bookkeeping.

## Version Compatibility

| bevy | ratatui | wgpu | bevy_tui_texture |
|------|---------|------|------------------|
| 0.19 | 0.30.2  | 29   | 0.3              |

Rust edition 2024, `rust-version = "1.96"` — tracks bevy 0.19's MSRV, not
this crate's own code (see README.md "MSRV policy").
