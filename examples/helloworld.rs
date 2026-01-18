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
    let font_data = font_handle
        .load()
        .expect("Failed to load font")
        .copy_font_data()
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
