# bevy_tui_texture

> A Bevy plugin for rendering terminal-style UIs using ratatui and WGPU

Seamlessly integrate terminal UIs into your Bevy applications. Display ratatui widgets on 2D sprites, 3D meshes, or UI elements with full GPU acceleration.

https://github.com/user-attachments/assets/57c2fb98-04a6-4ecf-8c72-58808a9f499f

## Features

- **GPU-Accelerated Terminal Rendering** - Render ratatui terminal UIs as GPU textures using WGPU
- **Flexible Display Options** - Render terminals on Bevy UI nodes, 2D sprites, or 3D meshes
- **Full Unicode Support** - Complete support for CJK (Chinese, Japanese, Korean) characters
- **Interactive Input** - Built-in keyboard and mouse input handling with focus management
- **Programmatic Glyphs** - Automatic rendering of box-drawing, block elements, and Braille patterns
- **Real-time Updates** - Efficient real-time terminal content updates with minimal overhead
- **Declarative Spawning** - `TuiRequest` component: spawn it, the plugin materializes the terminal - no render resources in your setup system

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.19"
font-kit = "0.14"
bevy_tui_texture = "0.3"
```

### Hello World Example

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

The `examples/` directory contains comprehensive demonstrations:

### Basic Examples

- **`helloworld.rs`** - Minimal example showing basic terminal rendering

### Widget Examples

- **`widget_catalog_2d.rs`** - Interactive showcase of ratatui widgets in 2D UI
- **`widget_catalog_3d.rs`** - Interactive showcase of ratatui widgets on rotating 3D mesh

### Advanced Examples

- **`multiple_terminals.rs`** - Managing multiple independent terminals
- **`world_terminal.rs`** - World-unit-sized in-game screen (`TuiRequest::world_quad` + `TuiFontSource::Asset`)
- **`shader_mesh.rs`** - Custom shader effects and mesh3d with terminal textures
- **`retro_crt.rs`** - glTF model + `ExtendedMaterial` CRT shader + overlay UI + camera modes
- **`tui_component.rs`** - Manual entity spawning with `TerminalTexture` (no spawn helpers)
- **`resize.rs`** - `Tui::request_resize` following the window size live
- **`transparent_world_quad.rs`** - HUD-style see-through screen (`transparent_reset_bg` + `AlphaMode::Blend`)
- **`benchmark.rs`** - Performance benchmarking and optimization

### WebAssembly

- **`wasm_demo.rs`** - the full retro CRT scene running in a browser (WebGL2)

## Run examples with

```bash
cargo run --example helloworld
cargo run --example widget_catalog_3d

# For the WASM demo (browser-ready site in docs/ - see docs/README.md
# for deploy + local preview instructions)
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli   # version must match Cargo.lock's wasm-bindgen

cargo build --example wasm_demo --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir docs \
  target/wasm32-unknown-unknown/wasm-release/examples/wasm_demo.wasm

# Shrink the binary (~31MB -> ~24MB) with wasm-opt (binaryen); see
# docs/README.md's "Binary size" section for what each flag does and why.
wasm-opt -Oz --strip-debug --strip-producers \
  --enable-nontrapping-float-to-int --enable-bulk-memory --enable-sign-ext \
  --enable-mutable-globals --enable-simd --enable-reference-types \
  -o docs/wasm_demo_bg.wasm docs/wasm_demo_bg.wasm
```

## Feature Flags

Configure features in your `Cargo.toml`:

```toml
[dependencies.bevy_tui_texture]
version = "0.3"
default-features = false
features = ["2d", "3d", "keyboard_input", "mouse_input"]
```

Available features:

- **`2d`** (default) - 2D UI terminals (`TuiUi`, `TuiKind::Ui`)
- **`3d`** (default) - 3D mesh terminals (`TuiKind::WorldQuad`, `AttachTerminal`, mesh raycasting)
- **`keyboard_input`** (default) - Enable keyboard event handling
- **`mouse_input`** (default) - Enable mouse event handling for both 2D UI and 3D mesh terminals
- **`bold_italic_fonts`** - Enable fake bold and italic font rendering support
- **`emoji`** - Enable emoji and extended Unicode character support (WIP)

`TuiRequest`'s `TuiKind` variants gate individually (`Ui` needs `2d`,
`WorldQuad` needs `3d`, `Headless` is always available); build with just
one display surface by disabling the other, e.g.
`features = ["3d", "keyboard_input", "mouse_input"]` for a 3D-only app.

## Performance

This library is designed for real-time rendering with:

- Efficient GPU texture updates
- Cached glyph rendering with text atlas
- Minimal CPU-GPU data transfer
- Smart dirty tracking for terminal cells

See `examples/benchmark.rs` for performance metrics.

## Requirements

- Rust 1.95 or later (2024 edition)
- Bevy 0.19
- Ratatui 0.30
- A GPU with WGPU support

**MSRV policy**: the declared `rust-version` tracks whichever dependency
needs the newest compiler - currently bevy 0.19 itself (`rust-version =
"1.95.0"`), not anything this crate's own code requires (edition 2024's
floor is 1.85; ratatui 0.30.2 declares 1.88.0). Bumping bevy/ratatui may
raise this floor further; there's no separate "N versions behind latest
stable" policy on top of that.

| `bevy` | `ratatui` | `wgpu` | `bevy_tui_texture` |
|--------|-----------|--------|--------------------|
| `0.19` | `0.30`    | `29`   | `0.3`              |
| `0.18` | `0.30`    | `27`   | `0.2`              |
| `0.17` | `0.29`    | `26`   | `0.1`              |

`wgpu` must exactly match the version bevy itself is pinned to (see the
comment above the `wgpu` dependency in `Cargo.toml`) - bump together.

## Font Licensing

The example/test fonts under `assets/fonts/` are SIL OFL 1.1-licensed, not
MIT (this crate's own `license = "MIT"` covers the Rust code only):
`Mplus1Code-Regular.ttf` (`assets/fonts/LICENSE/mplus1code.txt`) and
`fusion-pixel-10px-monospaced-ja.ttf` (`assets/fonts/OFL.txt` +
`assets/fonts/LICENSE/*.txt` for its bundled source fonts).

## Platform Support

- **Windows** - Full support
- **macOS** - Full support
- **Linux** - Full support

## WebAssembly Support

The library renders ratatui terminal UIs as GPU textures, which works in WebGL2 environments. The WASM demo showcases a interactive widget catalog running on a rotating 3D plane in your browser.

### What Works

- Bevy 0.19 + WGPU 29.0 (WebGL2 backend)
- Full ratatui widget rendering (Tabs, Lists, Charts, Gauges, etc.)
- Keyboard and mouse input handling
- 3D ray casting for mouse interaction on 3D meshes
- Programmatic glyphs (box-drawing, block elements, Braille)
- Real-time animations and updates
- Font embedding via `include_bytes!()`

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## License

- See [LICENSE](LICENSE)

## Acknowledgments

This library builds on the excellent work of:

- **[Bevy](https://bevyengine.org/)** - A refreshingly simple data-driven game engine
- **[ratatui](https://ratatui.rs/)** - A Rust library for cooking up terminal user interfaces
- **[WGPU](https://wgpu.rs/)** - Safe and portable GPU abstraction in Rust
- **[ratatui-wgpu](https://github.com/Jesterhearts/ratatui-wgpu)** - Ratatui WGPU backend that inspired this work
- **[rio](https://rioterm.com/)** - Beautiful glyph rendering

## Related Projects

- [bevy_egui](https://github.com/mvlabat/bevy_egui) - Egui integration for Bevy
- [egui_ratatui](https://github.com/gold-silver-copper/egui_ratatui) - Egui widget + ratatui backend
- [bevy_ui](https://github.com/bevyengine/bevy/tree/main/crates/bevy_ui) - Native Bevy UI
- [tui-rs](https://github.com/fdehau/tui-rs) - Original terminal UI library

