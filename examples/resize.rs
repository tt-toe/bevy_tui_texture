//! `Tui::request_resize` — following the window size live.
//!
//! Demonstrates the runtime-resize recipe (no auto-fit helper ships - see
//! the doc comment on `Tui::request_resize`):
//! 1. listen for `InputEvent::Resize { pixels }` (already broadcast to
//!    every terminal by the plugin's `window_resize_system`),
//! 2. convert to `cols`/`rows` using the same font metrics the terminal was
//!    created with,
//! 3. call `Tui::request_resize(cols, rows)` - no GPU work at the call site,
//!    applied on the next `gpu_flush_system` pass (one frame of latency).
//!
//! The terminal fills the window and shows a live cols×rows readout plus a
//! click counter, so resizing the window and then clicking proves both the
//! resize itself and the mouse coordinate mapping stay correct afterward
//! (`TerminalDimensions` is kept in sync by the plugin automatically).
//!
//! Run with: `cargo run --example resize`

use bevy::prelude::*;
use bevy_tui_texture::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;

/// Marker for the resizing terminal entity.
#[derive(Component)]
struct ResizingTerminal;

/// Font metrics needed to convert a window resize (pixels) into a grid
/// size (cols/rows) - the same `Arc<Fonts>` the terminal was created with.
#[derive(Resource)]
struct TerminalFonts(Arc<Fonts>);

#[derive(Resource, Default)]
struct Clicks {
    count: u32,
    last: Option<(u16, u16)>,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Tui::request_resize — follows the window size".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .init_resource::<Clicks>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (handle_resize, handle_clicks).in_set(TerminalSystemSet::UserUpdate),
        )
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

fn setup(mut commands: Commands) {
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let fonts = Arc::new(Fonts::new(
        TerminalFont::new(font_data).expect("Failed to parse font"),
        16,
    ));

    // Initial grid: an arbitrary starting size - the first window-resize
    // event (bevy fires one on startup) immediately corrects it to match
    // the actual window.
    commands.spawn((
        TuiRequest::ui(80, 25, fonts.clone()),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        ResizingTerminal,
    ));
    commands.spawn(Camera2d);
    commands.insert_resource(TerminalFonts(fonts));
}

/// The resize recipe: pixels -> cols/rows -> `request_resize`. Runs in
/// `UserUpdate`, same as any other draw-adjacent system - resizing takes
/// no render resources and does no GPU work at the call site.
fn handle_resize(
    mut events: MessageReader<TerminalEvent>,
    fonts: Res<TerminalFonts>,
    mut terminals: Query<&mut Tui, With<ResizingTerminal>>,
) {
    let Ok(mut term) = terminals.single_mut() else {
        return;
    };
    for event in events.read() {
        if let InputEvent::Resize { pixels } = &event.input {
            let cols = (pixels.x / fonts.0.min_width_px()).max(1) as u16;
            let rows = (pixels.y / fonts.0.height_px()).max(1) as u16;
            term.request_resize(cols, rows);
        }
    }
}

fn handle_clicks(mut events: MessageReader<TerminalEvent>, mut clicks: ResMut<Clicks>) {
    for event in events.read() {
        if let InputEvent::Mouse(m) = &event.input
            && matches!(m.kind, MouseEventKind::Down(_)) {
                clicks.count += 1;
                clicks.last = Some((m.column, m.row));
            }
    }
}

fn render_terminal(
    mut screens: Query<&mut Tui, With<ResizingTerminal>>,
    clicks: Res<Clicks>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let (cols, rows) = term.grid_size();
    let size_px = term.size_px();
    let last = match clicks.last {
        Some((col, row)) => format!("last click: col {col}, row {row}"),
        None => "click anywhere!".to_string(),
    };

    term.draw(|frame| {
        let text = Paragraph::new(vec![
            Line::from(format!("grid: {cols} cols x {rows} rows")),
            Line::from(format!("pixels: {}x{}", size_px.x, size_px.y)),
            Line::from(format!("clicks: {}", clicks.count)),
            Line::from(last),
            Line::from(""),
            Line::from("resize the window to see the grid follow it"),
        ])
        .style(Style::default().fg(RatatuiColor::Green))
        .alignment(Alignment::Center)
        .block(Block::bordered().title("Tui::request_resize"));
        frame.render_widget(text, frame.area());
    });
}
