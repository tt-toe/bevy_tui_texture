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

/// Marker for the terminal entity - its `Tui` is queried directly, no
/// wrapping Resource needed.
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

/// Declarative spawning: no render resources anywhere in this signature -
/// spawn a `TuiRequest` and the plugin materializes it next frame.
fn setup(mut commands: Commands) {
    let fonts = {
        let font_data = SystemSource::new()
            .select_best_match(&[FamilyName::Monospace], &Properties::new())
            .expect("No monospace font found on this system")
            .load()
            .expect("Failed to load font")
            .copy_font_data()
            .expect("Failed to copy font data");
        let font_data: &'static [u8] = Box::leak(font_data.to_vec().into_boxed_slice());
        Arc::new(Fonts::new(
            TerminalFont::new(font_data).expect("Failed to parse font"),
            16,
        ))
    };

    commands.spawn((
        TuiRequest::ui(80, 25, fonts).with_config(TerminalConfig {
            keyboard: false,
            mouse: false,
            ..default()
        }),
        Node::default(),
        HelloTerminal,
    ));
    commands.spawn(Camera2d);
}

fn render_terminal(mut screens: Query<&mut Tui, With<HelloTerminal>>) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    term.draw(|frame| {
        let area = frame.area();
        // Simple "Hello, World!" paragraph
        let text = Paragraph::new("Hello, World!")
            .style(Style::default().fg(RatatuiColor::Green).bold())
            .alignment(Alignment::Center)
            .block(Block::bordered().title("Minimal Example"));
        frame.render_widget(text, area);
    });
}
