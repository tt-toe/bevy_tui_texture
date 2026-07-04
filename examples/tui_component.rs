//! `Tui` — the ECS-native terminal Component
//!
//! Demonstrates the new architecture:
//! - a terminal is a plain `Tui` Component, spawned on an ordinary entity
//!   (no wrapping Resource, no `event.target == my_resource.entity()`
//!   comparisons — query for `&Tui`/`&mut Tui` like anything else),
//! - user drawing systems take **zero render-resource parameters**
//!   (`Tui::draw` only touches the ratatui buffer; the plugin's
//!   `gpu_flush_system`, registered automatically by `TerminalPlugin`,
//!   renders into the library-owned texture, and a render-world system
//!   copies it straight into whatever `Image`/`GpuImage` the material's
//!   bind group already references),
//! - this works identically for a custom (`ExtendedMaterial`-based)
//!   material type - **no plugin registration and no per-frame touching
//!   needed for any material type**, custom or `StandardMaterial`, since
//!   the render-world copy writes into the same GPU texture the material
//!   already samples.
//!
//! See also `world_terminal.rs`, which uses the same zero-render-resource
//! pattern for a single in-world screen built via `TerminalBundle::world_quad`.
//! Here, `update_screen_a`/`update_screen_b` take only what they actually
//! need for gameplay logic - no `render_device`, `render_queue`, `images`,
//! or `materials` in either signature.
//!
//! Run with: `cargo run --example tui_component`

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
use ratatui::style::{Color as TuiColor, Modifier, Style};
use ratatui::widgets::{Block, Gauge, Paragraph};
use std::sync::Arc;

const CAMERA_POS: Vec3 = Vec3::new(0.0, 2.0, 9.0);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Tui component — zero render-resource draw systems".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        // Only registration ScreenB's ExtendedMaterial needs: the ordinary
        // bevy MaterialPlugin, so its shader pipeline exists at all. No
        // terminal-specific plugin - the render-world GPU copy updates its
        // texture exactly the same way it updates ScreenA's StandardMaterial.
        .add_plugins(MaterialPlugin::<ScreenBMaterial>::default())
        .init_resource::<ClickCounts>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            handle_terminal_events.in_set(TerminalSystemSet::UserUpdate),
        )
        .add_systems(
            Update,
            (update_screen_a, update_screen_b).in_set(TerminalSystemSet::UserUpdate),
        )
        .run();
}

/// Trivial no-op extension: proves the render-world copy updates any
/// material's texture, not just `StandardMaterial`, without requiring a
/// custom WGSL shader (all shader hooks default to the base material's).
#[derive(Asset, AsBindGroup, Clone, Default, TypePath)]
struct NoExtension {}

impl MaterialExtension for NoExtension {}

type ScreenBMaterial = ExtendedMaterial<StandardMaterial, NoExtension>;

/// Marker for the StandardMaterial screen (left).
#[derive(Component)]
struct ScreenA;

/// Marker for the ExtendedMaterial screen (right).
#[derive(Component)]
struct ScreenB;

#[derive(Resource, Default)]
struct ClickCounts {
    a: u32,
    b: u32,
}

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut ext_materials: ResMut<Assets<ScreenBMaterial>>,
) {
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

    let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
    let font = TerminalFont::new(font_data).expect("failed to load font");
    let fonts = Arc::new(Fonts::new(font, 24));

    // --- Screen A: StandardMaterial, no plugin registration needed ---
    let texture_a = TerminalTexture::create(
        24,
        8,
        fonts.clone(),
        true,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("failed to create terminal texture (screen A)");
    let aspect_a = texture_a.width as f32 / texture_a.height as f32;
    let mesh_a = meshes.add(Plane3d::new(Vec3::Z, Vec2::new(1.2 * aspect_a, 1.2)));
    let material_a = std_materials.add(StandardMaterial {
        base_color_texture: Some(texture_a.image_handle()),
        unlit: true,
        alpha_mode: AlphaMode::Opaque,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let screen_a = commands
        .spawn((
            Mesh3d(mesh_a),
            MeshMaterial3d(material_a),
            Transform::from_xyz(-1.5, 0.0, 0.0),
            ScreenA,
        ))
        .id();
    commands.entity(screen_a).insert((
        Tui::from_texture_state(texture_a),
        TuiSurface { tui: screen_a },
        TerminalInput::default(),
    ));

    // --- Screen B: custom ExtendedMaterial, updated via the render-world copy ---
    let texture_b = TerminalTexture::create(
        24,
        8,
        fonts,
        true,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("failed to create terminal texture (screen B)");
    let aspect_b = texture_b.width as f32 / texture_b.height as f32;
    let mesh_b = meshes.add(Plane3d::new(Vec3::Z, Vec2::new(1.2 * aspect_b, 1.2)));
    let material_b = ext_materials.add(ScreenBMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture_b.image_handle()),
            unlit: true,
            alpha_mode: AlphaMode::Opaque,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: NoExtension {},
    });
    let screen_b = commands
        .spawn((
            Mesh3d(mesh_b),
            MeshMaterial3d(material_b),
            Transform::from_xyz(1.5, 0.0, 0.0),
            ScreenB,
        ))
        .id();
    commands.entity(screen_b).insert((
        Tui::from_texture_state(texture_b),
        TuiSurface { tui: screen_b },
        TerminalInput::default(),
    ));
}

/// Zero render-resource parameters: no RenderDevice, RenderQueue,
/// Assets<Image>, or Assets<StandardMaterial> in sight. gpu_flush_system
/// (registered by TerminalPlugin) handles all of that.
fn update_screen_a(
    mut screens: Query<&mut Tui, With<ScreenA>>,
    time: Res<Time>,
    clicks: Res<ClickCounts>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let elapsed = time.elapsed_secs();
    term.draw(|frame| {
        let outer = Block::bordered()
            .title(" Screen A: StandardMaterial ")
            .border_style(Style::default().fg(TuiColor::LightCyan));
        let inner = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());
        frame.render_widget(
            Paragraph::new(format!(
                "zero-plumbing draw system\nt {elapsed:>6.1}s  clicks {}",
                clicks.a
            ))
            .style(Style::default().fg(TuiColor::White).add_modifier(Modifier::BOLD)),
            inner,
        );
    });
}

/// Same zero-plumbing pattern, on a screen using a custom ExtendedMaterial.
/// No terminal-specific setup was needed at all beyond the ordinary bevy
/// `MaterialPlugin` registration in `main` - this draw system needs
/// nothing more than the StandardMaterial screen's.
fn update_screen_b(
    mut screens: Query<&mut Tui, With<ScreenB>>,
    time: Res<Time>,
    clicks: Res<ClickCounts>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let elapsed = time.elapsed_secs();
    let ratio = ((elapsed.sin() + 1.0) / 2.0) as f64;
    term.draw(|frame| {
        let outer = Block::bordered()
            .title(" Screen B: ExtendedMaterial ")
            .border_style(Style::default().fg(TuiColor::LightMagenta));
        let inner = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());
        let rows =
            ratatui::layout::Layout::vertical([Constraint2::Length(2), Constraint2::Min(1)])
                .split(inner);
        frame.render_widget(
            Paragraph::new(format!(
                "any material, zero touching\nclicks {}",
                clicks.b
            ))
            .style(Style::default().fg(TuiColor::White)),
            rows[0],
        );
        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label(format!("{:>3.0}%", ratio * 100.0))
                .gauge_style(Style::default().fg(TuiColor::LightMagenta)),
            rows[1],
        );
    });
}

use ratatui::layout::Constraint as Constraint2;

/// `event.target` is always the `Tui` entity - here `tui == surface`, so
/// this is the identity case, but the code is written exactly as it would
/// be for a terminal attached to an existing mesh via `AttachTerminal`.
fn handle_terminal_events(
    mut events: MessageReader<TerminalEvent>,
    mut clicks: ResMut<ClickCounts>,
    screens_a: Query<&Tui, With<ScreenA>>,
    screens_b: Query<&Tui, With<ScreenB>>,
) {
    for event in events.read() {
        if !matches!(event.event, TerminalEventType::MousePress { .. }) {
            continue;
        }
        if screens_a.get(event.target).is_ok() {
            clicks.a += 1;
        } else if screens_b.get(event.target).is_ok() {
            clicks.b += 1;
        }
    }
}
