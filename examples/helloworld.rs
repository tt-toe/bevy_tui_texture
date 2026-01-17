//! # Hello World - Minimal Terminal Example
//!
//! **The simplest possible bevy_tui_texture example** demonstrating basic terminal rendering.
//!
//! ## What This Example Shows
//!
//! - Creating a terminal with `SimpleTerminal2D::create_and_spawn()`
//! - Loading a TrueType font from embedded bytes
//! - Rendering ratatui widgets (Paragraph, Block) in the terminal
//! - Basic keyboard input handling (ESC to quit)
//! - Proper system ordering with `TerminalSystemSet`
//!
//! ## Running
//!
//! ```bash
//! cargo run --example helloworld
//! ```
//!
//! ## Controls
//!
//! - **ESC** - Quit application
//!
//! ## Code Structure
//!
//! 1. **Setup Phase** - Load font, create terminal, spawn camera
//! 2. **Input Phase** - Handle keyboard events (ESC to quit)
//! 3. **Render Phase** - Draw ratatui widgets to terminal texture
//!
//! ## Key Concepts
//!
//! - `SimpleTerminal2D` - Easiest way to create a 2D terminal in one call
//! - `draw_and_render()` - Combined draw + GPU upload in one method
//! - `TerminalSystemSet::Render` - Proper system ordering for terminal rendering

use std::sync::Arc;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};

use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;

use bevy_tui_texture::Font as TerminalFont;
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::setup::SimpleTerminal2D;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, handle_input.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

#[derive(Resource)]
struct MyTerminal {
    terminal: SimpleTerminal2D,
}

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    // Spawn camera
    // commands.spawn(Camera2d);
    // Load font
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    // Create terminal with SimpleTerminal2D - one line!
    let terminal = SimpleTerminal2D::create_and_spawn(
        80,         // 80 columns
        25,         // 25 rows
        fonts,      // Font configuration
        (0.0, 0.0), // Position at top-left
        true,       // Enable programmatic glyphs
        true,       // Enable keyboard input
        false,      // Disable mouse input
        &mut commands,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("Failed to create terminal");

    // Spawn camera
    commands.spawn(Camera2d);

    // Store terminal in resource
    commands.insert_resource(MyTerminal { terminal });
}

fn handle_input(keys: Res<ButtonInput<KeyCode>>, mut app_exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
    }
}

fn render_terminal(
    mut terminal_res: ResMut<MyTerminal>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    terminal_res
        .terminal
        .draw_and_render(&render_device, &render_queue, &mut images, |frame| {
            let area = frame.area();

            // Simple "Hello, World!" paragraph
            let text = Paragraph::new("Hello, World!")
                .style(Style::default().fg(RatatuiColor::Green).bold())
                .alignment(Alignment::Center)
                .block(Block::bordered().title("Minimal Example"));

            frame.render_widget(text, area);
        });
}
