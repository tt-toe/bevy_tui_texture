//! WorldTerminal3D — world-unit-sized terminal screens inside a game scene.
//!
//! Demonstrates the highest-level 3D API:
//! - quad sized in **world units** (height; width follows the texture aspect),
//! - oriented toward the camera via a facing direction,
//! - `update()` redraws AND handles `StandardMaterial` change detection
//!   internally — no marker component or material query in user code,
//! - in-world mouse picking (click the screen; the hit cell is displayed),
//! - `Font::from_vec` (runtime-loaded font bytes, no `include_bytes!`).
//!
//! Run with: `cargo run --example world_terminal`
//! (use `cargo run`, not the bare binary: asset/font paths resolve via
//! `CARGO_MANIFEST_DIR`.)

use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont; // disambiguate from bevy's Font
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color as TuiColor, Modifier, Style};
use ratatui::widgets::{Block, Gauge, Paragraph};
use std::sync::Arc;

const CAMERA_POS: Vec3 = Vec3::new(0.0, 3.0, 9.0);
const SCREEN_POS: Vec3 = Vec3::new(0.0, 2.4, -1.5);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "WorldTerminal3D — in-game screen".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .init_resource::<Clicks>()
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_cube)
        .add_systems(
            Update,
            handle_terminal_events.in_set(TerminalSystemSet::UserUpdate),
        )
        .add_systems(
            Update,
            update_screen
                .in_set(TerminalSystemSet::Render)
                .run_if(resource_exists::<Screen>),
        )
        .run();
}

/// The in-world terminal screen (owns wgpu state → resource, not component).
#[derive(Resource)]
struct Screen(WorldTerminal3D);

#[derive(Resource, Default)]
struct Clicks {
    count: u32,
    last: Option<(u16, u16)>,
}

#[derive(Component)]
struct Spinning;

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Scene: camera, light, a spinning cube the screen reports on.
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(CAMERA_POS).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.4, 0.0)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.7, 0.4),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Spinning,
    ));

    // Font: loaded at runtime — Font::from_vec keeps the bytes alive
    // internally (no include_bytes!, no Box::leak).
    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Mplus1Code-Regular.ttf"
    );
    let bytes = std::fs::read(font_path).expect("font file missing");
    let font = TerminalFont::from_vec(bytes).expect("invalid font");
    let fonts = Arc::new(Fonts::new(font, 32));

    // The in-world screen: 2.2 world units tall, tilted toward the camera.
    let screen = WorldTerminal3D::create_and_spawn(
        28,
        10,
        fonts,
        SCREEN_POS,
        CAMERA_POS - SCREEN_POS, // face the camera
        2.2,                     // height in world units
        true,                    // programmatic glyphs (borders)
        true,                    // mouse picking + keyboard focus
        &mut commands,
        &mut meshes,
        &mut materials,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("terminal creation failed");

    commands.insert_resource(Screen(screen));
}

fn rotate_cube(time: Res<Time>, mut cubes: Query<&mut Transform, With<Spinning>>) {
    for mut transform in cubes.iter_mut() {
        transform.rotate_y(time.delta_secs() * 0.8);
    }
}

/// Clicks on the screen arrive as regular TerminalEvents, picked through the
/// game camera's ray (multi-camera setups work too).
fn handle_terminal_events(mut events: MessageReader<TerminalEvent>, mut clicks: ResMut<Clicks>) {
    for event in events.read() {
        if let TerminalEventType::MousePress { position, .. } = &event.event {
            clicks.count += 1;
            clicks.last = Some(*position);
        }
    }
}

/// One call per frame: draw → GPU render → Image copy → material touch.
fn update_screen(
    mut screen: ResMut<Screen>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    clicks: Res<Clicks>,
    cubes: Query<&Transform, With<Spinning>>,
) {
    let elapsed = time.elapsed_secs();
    let yaw = cubes
        .single()
        .map(|t| t.rotation.to_euler(EulerRot::YXZ).0.to_degrees())
        .unwrap_or(0.0);
    let clicks_line = match clicks.last {
        Some((x, y)) => format!("clicks {}  last ({x},{y})", clicks.count),
        None => "click me!".to_string(),
    };

    screen.0.update(
        &render_device,
        &render_queue,
        &mut images,
        &mut materials,
        |frame| {
            let outer = Block::bordered()
                .title(" WorldTerminal3D ")
                .border_style(Style::default().fg(TuiColor::LightCyan));
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let rows = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(inner);

            frame.render_widget(
                Paragraph::new(format!(
                    "in-game screen\nt {elapsed:>6.1}s  cube yaw {yaw:>6.1}°\n{clicks_line}",
                ))
                .style(Style::default().fg(TuiColor::White).add_modifier(Modifier::BOLD)),
                rows[0],
            );

            let ratio = ((elapsed.sin() + 1.0) / 2.0) as f64;
            frame.render_widget(
                Gauge::default()
                    .ratio(ratio)
                    .label(format!("{:>3.0}%", ratio * 100.0))
                    .gauge_style(Style::default().fg(TuiColor::Magenta)),
                rows[2],
            );
        },
    );
}
