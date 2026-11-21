use bevy::prelude::*;
use bevy::text::DEFAULT_FONT_DATA;
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
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
    let Some(font) = TerminalFont::new(DEFAULT_FONT_DATA) else {
        error!("bevy's default font failed to parse");
        return;
    };
    let fonts = Arc::new(Fonts::new(font, 16));

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
        let text = Paragraph::new("Hello, World!")
            .style(Style::default().fg(RatatuiColor::Green).bold())
            .alignment(Alignment::Center)
            .block(Block::bordered().title("Minimal Example"));
        frame.render_widget(text, area);
    });
}
