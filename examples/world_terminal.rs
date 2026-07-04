//! `TerminalBundle::world_quad` — world-unit-sized terminal screen inside a
//! game scene, built on the `Tui` component.
//!
//! - quad sized in **world units** (height; width follows the texture aspect),
//! - orientation is an ordinary `Transform` in the spawn tuple (no `facing`
//!   parameter — see `TerminalBundle::world_quad`'s doc comment),
//! - `update_screen` below takes **zero render-resource parameters**;
//!   `Tui::draw` only touches the ratatui buffer, and `TerminalPlugin`'s
//!   `gpu_flush_system` owns the GPU render + async copy + material touch,
//! - in-world mouse picking (click the screen; the hit cell is displayed),
//! - the font is loaded through the `AssetServer` (works on native and Wasm
//!   alike) via the same "wait until loaded, then spawn" pattern every bevy
//!   user already writes for glTF.
//!
//! Run with: `cargo run --example world_terminal`
//! (use `cargo run`, not the bare binary: asset/font paths resolve via
//! `CARGO_MANIFEST_DIR`.)

use bevy::prelude::*;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color as TuiColor, Modifier, Style};
use ratatui::widgets::{Block, Gauge, Paragraph};

use bevy_tui_texture::prelude::*;

const CAMERA_POS: Vec3 = Vec3::new(0.0, 3.0, 9.0);
const SCREEN_POS: Vec3 = Vec3::new(0.0, 2.4, -1.5);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "TerminalBundle::world_quad — in-game screen".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .init_resource::<Clicks>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            spawn_screen_when_font_ready.run_if(resource_exists::<PendingFont>),
        )
        .add_systems(Update, rotate_cube)
        .add_systems(
            Update,
            handle_terminal_events.in_set(TerminalSystemSet::UserUpdate),
        )
        .add_systems(Update, update_screen.in_set(TerminalSystemSet::UserUpdate))
        .run();
}

/// Marker for the in-world screen entity (its `Tui` is queried directly).
#[derive(Component)]
struct Screen;

/// Font asset handle, kicked off in `setup` and polled by
/// `spawn_screen_when_font_ready` - removed once the screen is spawned.
#[derive(Resource)]
struct PendingFont(Handle<TerminalFontAsset>);

#[derive(Resource, Default)]
struct Clicks {
    count: u32,
    last: Option<(u16, u16)>,
}

#[derive(Component)]
struct Spinning;

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
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

    // Font: loaded through the AssetServer - works on native and Wasm
    // alike. The screen itself is spawned once this finishes loading, by
    // spawn_screen_when_font_ready below.
    let font: Handle<TerminalFontAsset> = asset_server.load("fonts/Mplus1Code-Regular.ttf");
    commands.insert_resource(PendingFont(font));
}

/// Waits for the font asset to finish loading, then spawns the in-world
/// screen. Runs every frame (via `run_if(resource_exists::<PendingFont>)`)
/// until it succeeds, then removes `PendingFont` so it stops running.
fn spawn_screen_when_font_ready(
    mut commands: Commands,
    pending: Res<PendingFont>,
    font_assets: Res<Assets<TerminalFontAsset>>,
    render_device: Res<bevy::render::renderer::RenderDevice>,
    render_queue: Res<bevy::render::renderer::RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(asset) = font_assets.get(&pending.0) else {
        return; // still loading
    };
    let fonts = Fonts::from_asset(asset, 32).expect("invalid font");

    let mut ctx = TerminalSpawnCtx {
        render_device: &render_device,
        render_queue: &render_queue,
        images: &mut images,
        meshes: &mut meshes,
        materials: &mut materials,
    };

    // The in-world screen: 2.2 world units tall, tilted toward the camera.
    let bundle = TerminalBundle::world_quad(28, 10, fonts, 2.2, TerminalConfig::default(), &mut ctx)
        .expect("terminal creation failed");

    commands.spawn((
        bundle,
        Transform::from_translation(SCREEN_POS)
            .with_rotation(Quat::from_rotation_arc(Vec3::Z, CAMERA_POS - SCREEN_POS)),
        Screen,
    ));
    commands.remove_resource::<PendingFont>();
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

/// Zero render-resource parameters — see module doc comment.
fn update_screen(
    mut screens: Query<&mut Tui, With<Screen>>,
    time: Res<Time>,
    clicks: Res<Clicks>,
    cubes: Query<&Transform, With<Spinning>>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let elapsed = time.elapsed_secs();
    let yaw = cubes
        .single()
        .map(|t| t.rotation.to_euler(EulerRot::YXZ).0.to_degrees())
        .unwrap_or(0.0);
    let clicks_line = match clicks.last {
        Some((x, y)) => format!("clicks {}  last ({x},{y})", clicks.count),
        None => "click me!".to_string(),
    };

    term.draw(|frame| {
        let outer = Block::bordered()
            .title(" TerminalBundle::world_quad ")
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
    });
}
