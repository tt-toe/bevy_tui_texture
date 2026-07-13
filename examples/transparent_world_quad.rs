//! Transparent `TuiKind::WorldQuad` — a HUD-style see-through screen (P2-2).
//!
//! Combines `TerminalConfig::alpha_mode: AlphaMode::Blend` with
//! `transparent_reset_bg: true`: cells with no explicit background
//! (`ratatui::style::Color::Reset`, ratatui's own default) render with
//! alpha 0, so the colorful spinning cube behind the terminal shows through
//! everywhere except the bordered panel (which sets an explicit
//! background) and the text itself.
//!
//! Run with: `cargo run --example transparent_world_quad`

use bevy::prelude::*;
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;

#[derive(Component)]
struct ScreenTerminal;

#[derive(Component)]
struct Spinning;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Transparent TuiKind::WorldQuad".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_cube)
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.4, 0.0)),
    ));

    // A colorful cube behind the screen - visible through the terminal's
    // transparent (Color::Reset) background cells once the screen is drawn.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.5, 2.5, 2.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: bevy::color::Color::srgb(0.9, 0.3, 0.2),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, -1.0),
        Spinning,
    ));

    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let fonts = Arc::new(Fonts::new(
        TerminalFont::new(font_data).expect("Failed to parse font"),
        24,
    ));

    commands.spawn((
        TuiRequest::world_quad(28, 12, fonts, 3.0).with_config(TerminalConfig {
            transparent_reset_bg: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        Transform::from_xyz(0.0, 0.0, 1.0),
        ScreenTerminal,
    ));
}

fn rotate_cube(time: Res<Time>, mut cubes: Query<&mut Transform, With<Spinning>>) {
    for mut transform in cubes.iter_mut() {
        transform.rotate_y(time.delta_secs() * 0.6);
        transform.rotate_x(time.delta_secs() * 0.3);
    }
}

fn render_terminal(mut screens: Query<&mut Tui, With<ScreenTerminal>>) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    term.draw(|frame| {
        let area = frame.area();
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).split(area);

        // An explicit background (not Reset) - stays opaque even with
        // transparent_reset_bg enabled, since only Color::Reset is affected.
        let panel = Paragraph::new("see the cube\nthrough the gaps")
            .style(
                Style::default()
                    .fg(RatatuiColor::White)
                    .bg(RatatuiColor::Rgb(20, 20, 60)),
            )
            .alignment(Alignment::Center)
            .block(
                Block::bordered()
                    .title(" transparent_reset_bg ")
                    .border_style(Style::default().fg(RatatuiColor::Cyan)),
            );
        frame.render_widget(panel, rows[0]);

        // No background set (Color::Reset, ratatui's default) - transparent.
        let floating_text = Paragraph::new("floating text\nno background at all")
            .style(Style::default().fg(RatatuiColor::Yellow).bold())
            .alignment(Alignment::Center);
        frame.render_widget(floating_text, rows[1]);
    });
}
