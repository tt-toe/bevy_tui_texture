---
name: bevy-tui-texture
description: >
  Render ratatui terminal UIs as GPU textures inside a Bevy app (2D UI
  nodes, 3D quads, or existing glTF meshes). Use this skill whenever code
  in this repo (or a consumer of the bevy_tui_texture crate) spawns,
  draws, resizes, or handles input for an in-game terminal — including
  WASM/WebGL2 builds. Covers the CURRENT declarative API (TuiRequest) —
  the API in most training data (TerminalBundle, TerminalSpawnCtx,
  material touching, RenderDevice parameters) is deleted; see "Stale
  patterns" below before writing code.
---

# bevy_tui_texture — agent usage guide

Versions: **bevy 0.19 + ratatui 0.30.2 + wgpu 29** (wgpu must exactly match
bevy's pin — bump together; the crate re-exports `wgpu` and `ratatui` so
downstream code can name matching types without pinning its own copies).

## Mental model

- A terminal is a **`Tui` component** on an ordinary entity. Query it like
  anything else (`Query<&mut Tui, With<MyMarker>>`).
- `tui.draw(|frame| ...)` is **pure CPU** — it fills ratatui's buffer and
  marks the component dirty only if cells actually changed (byte-identical
  redraws are free; calling it every frame is the intended pattern).
- The plugin renders dirty terminals **entirely in the render world**,
  directly into the destination `Image`'s `GpuImage` (one frame of
  latency). There is **no CPU readback, no GPU→GPU copy, and no material
  touching** anywhere — any material type (`StandardMaterial`,
  `ExtendedMaterial`, fully custom) updates automatically with zero
  registration.
- User systems never need `RenderDevice`, `RenderQueue`, `Assets<Image>`,
  `Assets<Mesh>`, or `Assets<StandardMaterial>` — not at spawn time, not
  per frame.
- Per-terminal GPU state (vertex/index buffers, screen-size uniform) is
  created lazily on first render and evicted automatically when the
  `Tui`'s last `Handle<Image>` drops. The glyph atlas + compositor
  pipelines are shared across every terminal using the same `Fonts`
  (keyed by font identity, not by terminal) and evicted once no live
  `Tui` uses that font anymore. No manual cleanup either way.

## Choosing a spawn API (top of the ladder first)

1. **`TuiRequest`** (default choice) — spawn the component, the plugin
   materializes the terminal on the next frame:
   - `TuiRequest::ui(cols, rows, fonts)` → bevy_ui terminal (`ImageNode` +
     required `Node`; your own `Node` in the spawn tuple wins).
   - `TuiRequest::world_quad(cols, rows, fonts, height)` → 3D quad sized
     in world units (height; width follows texture aspect). Face normal is
     local **`+Z`** — orient with `Transform::from_translation(pos)
     .with_rotation(Quat::from_rotation_arc(Vec3::Z, camera_pos - pos))`.
     `Transform::looking_at` is the OPPOSITE convention and shows the back.
   - `TuiRequest::headless(cols, rows, fonts)` → a `Tui` with no surface —
     for `AttachTerminal` targets (glTF screens) or fully custom
     meshes/materials.
   - Non-default config via `.with_config(TerminalConfig { ... })`.
2. **`AttachTerminal { terminal, material }`** — put a headless `Tui`'s
   content on an *existing* mesh entity (e.g. a glTF primitive). The system
   re-claims automatically until the async glTF loader stops overwriting
   the material, then goes idle (no archetype churn once settled).
   `AttachMaterial::standard(AlphaMode::Opaque)` or
   `::custom(|image| MyMaterial { ... })`.
3. **`TerminalTexture::create(cols, rows, fonts, programmatic_glyphs,
   transparent_reset_bg, initial_fill, &mut images)` +
   `Tui::from_texture_state(...)`** — low-level escape hatch only (see
   `examples/tui_component.rs`, `examples/shader_mesh.rs`).

For custom render passes / shaders, `tui.image_handle()` exposes the
destination `Handle<Image>`; look up its `GpuImage` in the render world
and use the re-exported `wgpu` types against it.

## Core recipes

Spawn (no render resources in the signature — this is the whole point):

```rust
fn setup(mut commands: Commands) {
    let fonts = Arc::new(Fonts::new(
        Font::new(include_bytes!("assets/fonts/Mplus1Code-Regular.ttf")).unwrap(),
        16, // cell height in px
    ));
    commands.spawn((TuiRequest::ui(80, 25, fonts), Node::default(), MyTerminal));
}
```

Per-frame draw (register in `TerminalSystemSet::Render` or `UserUpdate`):

```rust
fn draw(mut screens: Query<&mut Tui, With<MyTerminal>>) {
    let Ok(mut term) = screens.single_mut() else { return }; // REQUIRED:
    // TuiRequest materializes one frame after spawn — tolerate absence.
    term.draw(|frame| { /* ratatui widgets */ });
}
```

Runtime resize — `tui.request_resize(cols, rows)` (no GPU work at call
site; `Image` is recreated at the same handle, `TerminalDimensions` and
world-quad mesh aspect update automatically; same-size requests are
no-ops). No auto-fit helper exists by design: convert
`TerminalEventType::Resize` pixels via `fonts.min_width_px()` /
`fonts.height_px()` — full recipe in `examples/resize.rs`.

Transparency (HUD-style see-through screen):

```rust
TuiRequest::world_quad(28, 12, fonts, 3.0).with_config(TerminalConfig {
    transparent_reset_bg: true,          // Color::Reset cells → alpha 0
    alpha_mode: AlphaMode::Blend,        // 3d feature; WorldQuad material
    ..default()
})
```

Only `ratatui::style::Color::Reset` backgrounds (ratatui's default) become
transparent; explicit background colors stay opaque. `initial_fill:
[u8; 4]` sets the pre-first-draw clear color (default opaque black);
`initial_draw: Some(Box::new(|frame| ...))` draws real content before the
first presented frame (closure must be `+ Send + Sync`).

Fonts:
- Native embedded: `Font::new(include_bytes!(...))` (needs `'static`).
- Runtime bytes: `Font::from_vec(vec)` — never `Box::leak` for this.
- Async/Wasm-safe: `TuiFontSource::Asset { handle:
  asset_server.load("fonts/x.ttf"), size_px: 16 }` — the request stays
  pending until the `.ttf` loads, then materializes. (Named
  `TuiFontSource`, not `FontSource` — bevy's prelude already has a
  `FontSource` and glob imports would collide.)
- CJK/fallback: `fonts.add_regular_fonts([...])` etc.; the
  `bold_italic_fonts` feature enables real bold/italic font slots
  (`add_bold_fonts`/`add_italic_fonts`/`add_bold_italic_fonts`), otherwise
  bold/italic are faked from regular. The `emoji` feature enables
  emoji-aware glyph handling.

## Input, focus, and hit testing

Read `MessageReader<TerminalEvent>` (bevy messages, NOT legacy events);
`event.target` is always the `Tui` entity, even for attached surfaces.
Variants: `KeyPress { key, modifiers }`, `CharInput { character }`,
`MousePress { button, position }`, `MouseRelease { button, position }`,
`MouseMove { position }`, `FocusGained`, `FocusLost`,
`Resize { new_size: (px, px) }`.

- **`position` is `(col, row)`** — grid cells, x-then-y. (Some in-source
  doc comments still say "(row, col)"; the emitting code and `hit_at`
  agree on `(col, row)`. Trust this, not those comments.)
- **Keyboard needs focus**: only the focused terminal receives
  `KeyPress`/`CharInput`. Clicking a terminal focuses it
  (`TerminalInputConfig::focus_button`, default left); Tab cycles focus
  when `auto_focus` is on. Mouse events need no focus.
- **Plugin constructors**: `TerminalPlugin::default()` (all input),
  `::new(TerminalInputConfig { keyboard_enabled, mouse_enabled,
  auto_focus, focus_button })`, `::without_keyboard()`,
  `::without_mouse()`, `::display_only()`.
- **Multi-camera picking** works out of the box: rays are built per
  active camera via `Camera::viewport_to_world` (correct for every
  projection/`ScalingMode`), prioritized by descending `Camera::order`,
  then hit distance.
- **When a 2D UI terminal and a 3D mesh terminal overlap in screen
  space, the UI terminal wins** — bevy_ui renders as an overlay on top of
  the 3D view, so the pick order matches what the user sees.

Per-widget hit testing: `tui.draw_with_hits(|frame, hits| {
hits.add(id, rect); ... })` then
`tui.hit_regions().hit_at::<MyId>((col, row))` (topmost/last wins; a
failed id decode returns `None`, it does not fall through).

## WASM / WebGL2 (hard-won — read before shipping a browser build)

- **Your app's bevy dependency MUST enable the `bevy_winit` feature on
  wasm32.** On native it rides in via `x11`/`wayland`; a wasm feature
  list that drops those loses the event-loop runner itself. Symptom:
  the wasm loads, then the tab busy-loops forever with ZERO console
  output ("Script terminated by timeout", stack in
  `bevy_app::App::plugins_state`) — `App::run()`'s fallback runner spins
  waiting for a renderer-init future that can only progress on the JS
  microtask queue it is blocking.
- **Force conservative limits**: build the app with
  `WgpuSettingsPriority::WebGL2` (see `examples/retro_crt.rs`). The
  default `Functionality` priority trusts raw adapter limits + enables
  experimental features, which has produced unrecoverable startup traps
  on WebGL2.
- **Keep the canvas ≤ 2048 physical pixels per dimension.** WebGL2's
  guaranteed `max_texture_dimension_2d` is 2048, and
  `fit_canvas_to_parent` multiplies CSS size by `devicePixelRatio` — on
  Retina (DPR 2) a ~1100 CSS-px window already exceeds the cap, the
  surface fails validation, and bevy 0.19 quits to a black screen.
  Clamp the canvas's CSS `max-width`/`max-height` to
  `2048 / devicePixelRatio` (see `clamp_canvas_to_safe_texture_size` in
  `examples/wasm_demo.rs`). Spoofing `window.devicePixelRatio` via
  `Object.defineProperty` does NOT work — winit measures via
  `ResizeObserver`'s `devicePixelContentBoxSize`, which bypasses the JS
  property.
- The **glyph atlas is already WebGL2-safe by construction** (2048×2048,
  exactly the cap — same on native and wasm, so it has no wasm-only
  failure mode). The per-terminal destination texture is
  `cols·char_w × rows·char_h`; staying under the GPU cap for huge
  grid/font combinations is the caller's responsibility.
- **Suppressing browser-hostile keys** (e.g. an Escape handler that
  quits): register your own DOM `keydown` listener on the canvas BEFORE
  `App::run()` and call `stop_immediate_propagation()` — same-target DOM
  listeners fire in registration order, so winit (which registers its
  listener lazily at window creation) never sees the key. Pattern in
  `examples/wasm_demo.rs::suppress_canvas_escape_key`.

## GPU facts worth knowing

- Glyph atlas: 2048×2048 RGBA8, LRU-evicted; capacity is logged at INFO on
  creation ("Glyph atlas: 2048x2048px in use, entries WxHpx, capacity N").
  Entry slots are `2·min_width_px × height_px`, so capacity scales with
  font size. Shared across every terminal using the same `Fonts` — sharing
  a font across terminals rasterizes and uploads each glyph only once.
- Atlas textures are explicitly zero-initialized at creation — no WebGL
  "lazy initialization" warnings from partial glyph uploads.
- All terminal textures are `Rgba8Unorm`; the render is a two-pass
  (background quads, then glyphs) directly into the destination
  `GpuImage::texture_view`.

## Invariants agents must not break

- **System ordering**: user draw systems go in
  `TerminalSystemSet::UserUpdate` or `::Render`; input handling reads in
  `UserUpdate`. The plugin chains Input → UserUpdate → Render in `Update`.
- **One-frame materialization latency**: anything spawned via `TuiRequest`
  has no `Tui` until the next frame. Every consumer query must use the
  `let Ok(..) = q.single_mut() else { return }` pattern.
- **Custom material on a world quad** → use `TuiKind::Headless` + your own
  mesh entity. `insert_if_new` only suppresses the same component type;
  a custom `MeshMaterial3d<M>` would coexist with the generated
  `StandardMaterial` one.
- **`Tui::read_back_blocking(&channel)`** (screenshots/tests only, never
  per frame) takes `Res<TuiReadbackChannel>` and blocks on the render
  world — call it from a different thread than the one driving
  `App::update()` unless `PipelinedRenderingPlugin` is active.
- **glTF attach targets**: primitive entities are named
  `<MESH>.<MATERIAL>` (e.g. `Object_2.Monitor_Glass`), not after the node;
  exact-match, never prefix-match (`Object_2` also hits `Object_20`).
- **Feature gates**: `2d`/`3d`/`keyboard_input`/`mouse_input` (all
  default), plus opt-in `bold_italic_fonts` and `emoji`. `TuiKind::Ui`
  needs `2d`, `WorldQuad` needs `3d`, `Headless` is always available.

## Stale patterns (training-data API — DELETED, do not write these)

| If you were about to write… | Write instead |
|---|---|
| `TerminalBundle` / `TerminalSpawnCtx` | `TuiRequest::ui/world_quad/headless` |
| `materials.get_mut(handle)` "touch" every frame | Nothing — updates are automatic |
| `Res<RenderDevice>` / `Res<RenderQueue>` in setup or draw systems | Plain `Commands` / `Query<&mut Tui>` |
| `terminal.draw()` + `render_to_texture()` + copy-back | `tui.draw(...)` only; the plugin owns the GPU path |
| Per-material-type plugin registration for texture updates | Nothing — works for any `Material` impl |
| `EventReader<TerminalEvent>` | `MessageReader<TerminalEvent>` (bevy 0.18+ messages) |
| `Font::new(bytes.leak())` for runtime fonts | `Font::from_vec(bytes)` |
| 1800×1200 atlas constants | 2048×2048 (`CACHE_WIDTH`/`CACHE_HEIGHT`) |

## Verification (what CI runs — .github/workflows/ci.yml)

```bash
cargo test --all-features                       # 60 tests, GPU-free except 1 skip-capable
cargo clippy --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo check --lib --no-default-features         # + --features 2d / 3d variants
cargo run --example helloworld                  # ALWAYS cargo run, never the bare
                                                # binary (assets resolve via CARGO_MANIFEST_DIR)
```

WASM build pipeline (browser demo → `examples/web/`): see the header of
`examples/wasm_demo.rs` (cargo build --profile wasm-release →
wasm-bindgen → wasm-opt; bump `ASSET_VERSION` in `examples/web/index.html`
on every redeploy — the JS glue and wasm are a matched pair and stale
browser caches mixing versions fail with `LinkError`). Assets (models,
shaders, fonts) live in `examples/assets/`, shared by native and wasm —
`examples/web/index.html` fetches them from the sibling `../assets/` at
runtime, so local preview must serve from `examples/` (not
`examples/web/` itself), see `examples/web/README.md`.

Deeper reference: `examples/` (one per feature — `resize.rs`, `transparent_world_quad.rs`,
`world_terminal.rs` for async fonts, `retro_crt.rs` for glTF attach,
`wasm_demo.rs` for the browser shim).
