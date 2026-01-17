# bevy_tui_texture

> A Bevy plugin for rendering terminal-style UIs using ratatui and WGPU

Seamlessly integrate terminal UIs into your Bevy applications. Display ratatui widgets on 2D sprites, 3D meshes, or UI elements with full GPU acceleration.

## Features

- **GPU-Accelerated Terminal Rendering** - Render ratatui terminal UIs as GPU textures using WGPU
- **Flexible Display Options** - Render terminals on Bevy UI nodes, 2D sprites, or 3D meshes
- **Full Unicode Support** - Complete support for CJK (Chinese, Japanese, Korean) characters
- **Interactive Input** - Built-in keyboard and mouse input handling with focus management
- **Programmatic Glyphs** - Automatic rendering of box-drawing, block elements, and Braille patterns
- **Real-time Updates** - Efficient real-time terminal content updates with minimal overhead
- **Easy Setup API** - Simple `SimpleTerminal2D` and `SimpleTerminal3D` helpers for quick integration
- **Bevy 0.17 Compatible** - Built for the latest Bevy version with modern ECS patterns

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy_tui_texture = "0.1.0"
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
    // Load font
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
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

- **`wasm_demo.rs`** - Full widget catalog running in browser (see [WebAssembly Support](#webassembly-support))
- **`web_server.rs`** - Local development server for WASM demo

Run any example with:

```bash
cargo run --example helloworld
cargo run --example widget_catalog_3d

# For WASM demo
cargo wasm && cargo run --example web_server
```

## Architecture

The library is organized into several key modules:

- **`bevy_plugin`** - Core Bevy plugin, resources, and components
- **`backend`** - WGPU-based ratatui backend implementation
- **`setup`** - Simplified setup utilities (`SimpleTerminal2D`, `SimpleTerminal3D`)
- **`fonts`** - Font loading and rendering with Unicode support
- **`input`** - Keyboard and mouse input handling system
- **`colors`** - ANSI color conversion and management
- **`utils`** - Text atlas and rendering utilities

## Feature Flags

Configure features in your `Cargo.toml`:

```toml
[dependencies.bevy_tui_texture]
version = "0.1.1"
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

- Rust 1.75 or later (2024 edition)
- Bevy 0.17.3
- A GPU with WGPU support

| `bevy` | `ratatui` | `bevy_tui_texture` |
|--------|-----------|--------------------|
| `0.17` | `0.29`    | `0.1`              |

## Platform Support

- **Windows** - Full support
- **macOS** - Full support
- **Linux** - Full support

## WebAssembly Support

### Status

**✅ Fully Functional** - bevy_tui_texture works in browsers via WebAssembly!

The library renders ratatui terminal UIs as GPU textures, which works perfectly in WebGL2 environments. The WASM demo showcases a complete interactive widget catalog running on a rotating 3D plane in your browser.

### What Works

- ✅ Bevy 0.17 + WGPU 26.0.1 (WebGL2 backend)
- ✅ Full ratatui widget rendering (Tabs, Lists, Charts, Gauges, etc.)
- ✅ Keyboard and mouse input handling
- ✅ 3D ray casting for mouse interaction on 3D meshes
- ✅ Programmatic glyphs (box-drawing, block elements, Braille)
- ✅ Real-time animations and updates
- ✅ Font loading via `include_bytes!()`

### Building for WebAssembly

#### Prerequisites

```bash
# Add wasm target
rustup target add wasm32-unknown-unknown

# Install wasm-bindgen CLI
cargo install wasm-bindgen-cli

# Install wasm-opt (via binaryen)
# macOS: brew install binaryen
# Ubuntu: apt install binaryen

# Install wabt for wasm-strip
# macOS: brew install wabt
# Ubuntu: apt install wabt
```

#### Build WASM

```bash
cargo wasm
```

This executes:
1. `cargo build --target wasm32-unknown-unknown --profile wasm-release --bin wasm_demo`
2. `wasm-bindgen` - Generate JS bindings
3. `wasm-opt` - Optimize for size (-Oz)
4. `wasm-strip` - Strip debug symbols

Output files are placed in `examples/web/`.

#### Serve Locally

```bash
# Start the web server
cargo run --example web_server

# Open browser to http://127.0.0.1:8080
```

The web server serves the WASM demo with proper CORS headers for WebAssembly.

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## License

- MIT License ([LICENSE](LICENSE) or http://opensource.org/licenses/MIT)

## Acknowledgments

This library builds on the excellent work of:

- **[Bevy](https://bevyengine.org/)** - A refreshingly simple data-driven game engine
- **[ratatui](https://ratatui.rs/)** - A Rust library for cooking up terminal user interfaces
- **[WGPU](https://wgpu.rs/)** - Safe and portable GPU abstraction in Rust
- **[ratatui-wgpu](https://github.com/joshka/ratatui-wgpu)** - The original ratatui WGPU backend that inspired this work
- **[rio](https://rioterm.com/)** - Beautiful glyph rendering

## Related Projects

- [bevy_egui](https://github.com/mvlabat/bevy_egui) - Egui integration for Bevy
- [bevy_ui](https://github.com/bevyengine/bevy/tree/main/crates/bevy_ui) - Native Bevy UI
- [tui-rs](https://github.com/fdehau/tui-rs) - Original terminal UI library

---

Made with Bevy and ratatui
