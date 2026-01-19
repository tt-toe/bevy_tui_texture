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
- **Easy Setup API** - Simple `SimpleTerminal2D` and `SimpleTerminal3D` helpers for quick integration

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.18"
font-kit = "0.14"
bevy_tui_texture = "0.2"
```

### Hello World Example

```rust
use std::sync::Arc;
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::style::Color as RatatuiColor;
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
use font_kit::source::SystemSource;
use font_kit::family_name::FamilyName;
use font_kit::properties::Properties;

#[derive(Resource)]
struct Terminal(SimpleTerminal2D);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    // Load system monospace font
    let font_handle = SystemSource::new()
        .select_best_match(&[FamilyName::Monospace], &Properties::new())
        .expect("No monospace font found on system");
    let font_data = font_handle.load().expect("Failed to load font").copy_font_data()
        .expect("Failed to copy font data");
    // Leak the font data to get a 'static reference (fine for app-lifetime fonts)
    let font_data: &'static [u8] = Box::leak(font_data.to_vec().into_boxed_slice());
    let font = TerminalFont::new(font_data).expect("Failed to parse font");
    let fonts = Arc::new(Fonts::new(font, 16));
    // Create terminal
    let terminal = SimpleTerminal2D::create_and_spawn(
        80, 25, fonts, (0.0, 0.0), true, false, false,
        &mut commands, &render_device, &render_queue, &mut images,
    ).expect("Failed to create terminal");
    // Spawn camera
    commands.spawn(Camera2d);
    commands.insert_resource(Terminal(terminal));
}

fn render_terminal(
    mut terminal: ResMut<Terminal>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    terminal.0.draw_and_render(&render_device, &render_queue, &mut images, |frame| {
            let area = frame.area();
            // Simple "Hello, World!" paragraph
            let text = Paragraph::new("Hello, World!")
                .style(Style::default().fg(RatatuiColor::Green).bold())
                .alignment(Alignment::Center)
                .block(Block::bordered().title("Minimal Example"));
            frame.render_widget(text, area);
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
- **`shader.rs`** - Custom shader effects with terminal textures
- **`benchmark.rs`** - Performance benchmarking and optimization

### WebAssembly

- **`wasm_demo.rs`** - Full widget catalog running in browser
- **`wasm_serve.rs`** - One-command WASM build & serve

## Run examples with

```bash
cargo run --example helloworld
cargo run --example widget_catalog_3d

# For WASM demo
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
# Install wasm-opt (via binaryen)
# macOS: brew install binaryen
# Ubuntu: apt install binaryen

# Install wabt for wasm-strip
# macOS: brew install wabt
# Ubuntu: apt install wabt
cargo run --example wasm_serve

# Output files are placed in `examples/web/`.
```

## Feature Flags

Configure features in your `Cargo.toml`:

```toml
[dependencies.bevy_tui_texture]
version = "0.2"
default-features = false
features = ["keyboard_input", "mouse_input", "ratatui_backend"]
```

Available features:

- **`keyboard_input`** (default) - Enable keyboard event handling
- **`mouse_input`** (default) - Enable mouse event handling for both 2D UI and 3D mesh terminals
- **`ratatui_backend`** (default) - Enable ratatui

## Performance

This library is designed for real-time rendering with:

- Efficient GPU texture updates
- Cached glyph rendering with text atlas
- Minimal CPU-GPU data transfer
- Smart dirty tracking for terminal cells

See `examples/benchmark.rs` for performance metrics.

## Requirements

- Rust 1.92 or later (2024 edition)
- Bevy 0.18
- Ratatui 0.30
- A GPU with WGPU support

| `bevy` | `ratatui` | `bevy_tui_texture` |
|--------|-----------|--------------------|
| `0.18` | `0.30`    | `0.2`              |
| `0.17` | `0.29`    | `0.1`              |

## Platform Support

- **Windows** - Full support
- **macOS** - Full support
- **Linux** - Full support

## WebAssembly Support

The library renders ratatui terminal UIs as GPU textures, which works in WebGL2 environments. The WASM demo showcases a interactive widget catalog running on a rotating 3D plane in your browser.

### What Works

- Bevy 0.18 + WGPU 27.0.1 (WebGL2 backend)
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

