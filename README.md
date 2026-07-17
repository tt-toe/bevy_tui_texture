# bevy_tui_texture

> Render ratatui terminal UIs as GPU textures in Bevy — on 2D UI nodes, 3D meshes, or existing glTF screens.

**[WASM DEMO](https://tt-toe.github.io/bevy_tui_texture/examples/web/)**

https://github.com/user-attachments/assets/64e1e136-7d2d-4e32-9c10-1da2cfb78ccd

## Features

- **GPU-accelerated rendering** — terminal content is drawn entirely in the render world, directly into the destination texture; no CPU readback, no per-frame material updates
- **Declarative spawning** — spawn a `TuiRequest` component and the plugin materializes the terminal; no render resources in your systems
- **Flexible display targets** — Bevy UI nodes, world-unit 3D quads, or attach to an existing mesh (e.g. a glTF screen) via `AttachTerminal`
- **Interactive input** — keyboard, mouse, and touch (taps emulate left-click) with focus management and per-widget hit testing
- **Full Unicode** — CJK support, font fallback chains, and procedural box-drawing / block / Braille / powerline glyphs
- **Efficient updates** — dirty-cell tracking, partial row redraws, and a glyph atlas shared across terminals using the same fonts
- **WebAssembly** — runs in the browser on WebGL2

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.19"
font-kit = "0.14"
bevy_tui_texture = "0.3"
```

### Hello World

Mirrors `examples/helloworld.rs` (also embedded as this crate's Quick Start doctest):

```rust
use bevy::prelude::*;
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
use font_kit::family_name::FamilyName;
use font_kit::properties::Properties;
use font_kit::source::SystemSource;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;

#[derive(Component)]
struct HelloTerminal;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

// No render resources anywhere in this signature - spawn a `TuiRequest`
// and the plugin materializes it next frame.
fn setup(mut commands: Commands) {
    let fonts = {
        let font_data = SystemSource::new()
            .select_best_match(&[FamilyName::Monospace], &Properties::new())
            .expect("No monospace font found on this system")
            .load().expect("Failed to load font").copy_font_data()
            .expect("Failed to copy font data");
        let font_data: &'static [u8] = Box::leak(font_data.to_vec().into_boxed_slice());
        Arc::new(Fonts::new(
            TerminalFont::new(font_data).expect("Failed to parse font"), 16))
    };

    commands.spawn((
        TuiRequest::ui(80, 25, fonts).with_config(TerminalConfig {
            keyboard: false, mouse: false, ..default()
        }),
        Node::default(),
        HelloTerminal,
    ));
    commands.spawn(Camera2d);
}

fn render_terminal(mut screens: Query<&mut Tui, With<HelloTerminal>>) {
    let Ok(mut term) = screens.single_mut() else { return };
    term.draw(|frame| {
        let text = Paragraph::new("Hello, World!")
            .style(Style::default().fg(RatatuiColor::Green).bold())
            .alignment(Alignment::Center)
            .block(Block::bordered().title("Minimal Example"));
        frame.render_widget(text, frame.area());
    });
}
```

## Examples

| Example | Shows |
|---|---|
| `helloworld.rs` | Minimal static terminal |
| `widget_catalog_2d.rs` | ratatui widget showcase in 2D UI with mouse interaction |
| `widget_catalog_3d.rs` | Widget showcase on a rotating 3D mesh |
| `multiple_terminals.rs` | Several independent terminals + Tab focus cycling |
| `world_terminal.rs` | World-unit in-game screen (`TuiRequest::world_quad` + `TuiFontSource::Asset`) |
| `shader_mesh.rs` | Custom shader effects on a terminal texture |
| `retro_crt.rs` | glTF model + `ExtendedMaterial` CRT shader + overlay UI + camera modes |
| `tui_component.rs` | Manual spawning with `TerminalTexture` (no helpers) |
| `resize.rs` | `Tui::request_resize` following the window size live |
| `transparent_world_quad.rs` | HUD-style see-through screen (`transparent_reset_bg` + `AlphaMode::Blend`) |
| `benchmark.rs` | Full-frame rendering throughput |
| `benchmark_partial.rs` | `BENCH_MODE=static\|partial` — unchanged-frame and partial-row redraw costs |
| `wasm_demo.rs` | The full retro CRT scene running in a browser (WebGL2) |

```bash
cargo run --example helloworld
cargo run --example retro_crt

# WASM demo (browser-ready site in examples/web/ - see
# examples/web/README.md for deploy + local preview instructions)
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli   # version must match Cargo.lock's wasm-bindgen

cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir examples/web \
  target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm

# Shrink the binary (~31MB -> ~24MB) with wasm-opt (binaryen); see
# examples/web/README.md's "Binary size" section for what each flag does and why.
wasm-opt -Oz --strip-debug --strip-producers \
  --enable-nontrapping-float-to-int --enable-bulk-memory --enable-sign-ext \
  --enable-mutable-globals --enable-simd --enable-reference-types \
  -o examples/web/wasm_demo_bg.wasm examples/web/wasm_demo_bg.wasm
```

## Feature Flags

```toml
[dependencies.bevy_tui_texture]
version = "0.3"
default-features = false
features = ["3d", "keyboard_input", "mouse_input"]   # e.g. a 3D-only app
```

- **`2d`** (default) — 2D UI terminals (`TuiUi`, `TuiKind::Ui`)
- **`3d`** (default) — 3D mesh terminals (`TuiKind::WorldQuad`, `AttachTerminal`, mesh raycasting)
- **`keyboard_input`** (default) — keyboard event handling
- **`mouse_input`** (default) — mouse events for 2D UI and 3D mesh terminals; touch taps ride the same path (a tap emulates a left-button click at the touch position)
- **`bold_italic_fonts`** — real bold/italic font slots; without it, bold/italic are faked from the regular font
- **`emoji`** — emoji and extended Unicode support (WIP)
- **`ascii_fast_shaping`** — skip text shaping for all-ASCII rows (assumes zero glyph offsets, true for most monospace fonts; inert when `bold_italic_fonts` is enabled)

`TuiKind` variants gate individually: `Ui` needs `2d`, `WorldQuad` needs `3d`, `Headless` is always available.

## Performance

- Dirty-cell tracking: byte-identical redraws cost nothing, and redraws touching only a few rows upload only those rows
- Glyph atlas shared across terminals using the same fonts — each glyph is rasterized and uploaded once
- Persistent, grow-only GPU buffers; all terminal draws ride the frame's single batched submit alongside the camera passes
- Terminal content lands in the same frame it is drawn (no one-frame lag)

See `examples/benchmark.rs` and `examples/benchmark_partial.rs`.

## Requirements

- Rust 1.96+ (2024 edition), Bevy 0.19, Ratatui 0.30, a GPU with WGPU support
- Platforms: Windows, macOS, Linux, and Web (WebGL2)
- Fonts must be TrueType (`.ttf`)

**MSRV policy**: the declared `rust-version` tracks whichever dependency
needs the newest compiler — currently bevy 0.19 itself, not anything this
crate's own code requires (edition 2024's floor is 1.85; ratatui 0.30.2
declares 1.88.0). Bumping bevy/ratatui may raise this floor; there is no
separate "N versions behind stable" policy on top.

| `bevy` | `ratatui` | `wgpu` | `bevy_tui_texture` |
|--------|-----------|--------|--------------------|
| `0.19` | `0.30`    | `29`   | `0.3`              |
| `0.18` | `0.30`    | `27`   | `0.2`              |
| `0.17` | `0.29`    | `26`   | `0.1`              |

`wgpu` must exactly match the version bevy itself pins (see the comment
above the `wgpu` dependency in `Cargo.toml`) — bump together.

## Font Licensing

The example/test fonts under `examples/assets/fonts/` are SIL OFL
1.1-licensed, not MIT (this crate's `license = "MIT"` covers the Rust code
only): `Mplus1Code-Regular.ttf`
(`examples/assets/fonts/LICENSE/mplus1code.txt`) and
`fusion-pixel-10px-monospaced-ja.ttf` (`examples/assets/fonts/OFL.txt` +
`examples/assets/fonts/LICENSE/*.txt` for its bundled source fonts).

## Contributing

Contributions are welcome! Feel free to submit issues or pull requests.

## License

- See [LICENSE](LICENSE)

## Acknowledgments

- **[Bevy](https://bevyengine.org/)** — a refreshingly simple data-driven game engine
- **[ratatui](https://ratatui.rs/)** — terminal user interfaces in Rust
- **[WGPU](https://wgpu.rs/)** — safe and portable GPU abstraction
- **[ratatui-wgpu](https://github.com/Jesterhearts/ratatui-wgpu)** — the WGPU backend that inspired this work
- **[rio](https://rioterm.com/)** — beautiful glyph rendering

## Related Projects

- [bevy_egui](https://github.com/mvlabat/bevy_egui) — Egui integration for Bevy
- [egui_ratatui](https://github.com/gold-silver-copper/egui_ratatui) — Egui widget + ratatui backend
- [bevy_ui](https://github.com/bevyengine/bevy/tree/main/crates/bevy_ui) — native Bevy UI
