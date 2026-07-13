// Benchmark Partial Example - Isolates the two flush-skipping optimizations
// (A1: unchanged-frame early-out, A2: row-incremental reshape) that
// examples/benchmark.rs cannot show, because that benchmark redraws every
// cell every frame.
//
// See IMPROVEMENT.md, item M1.
//
// Usage:
//   BENCH_MODE=static  cargo run --release --example benchmark_partial   # isolates A1
//   BENCH_MODE=partial cargo run --release --example benchmark_partial  # isolates A2
//   (default: static)
//
// Mode "static": the terminal content is a fixed, purely deterministic
// function of (row, col) - no frame count, no time - so every draw after
// the first produces a byte-identical ratatui buffer diff (empty). The
// user-side cost of recomputing that content every frame is deliberately
// left in place and constant across before/after runs, so any FPS/frame
// time delta measured here comes only from BevyTerminalBackend::flush()'s
// early-out (or lack of it).
//
// Mode "partial": identical static content, except exactly one row shows
// a live frame counter and therefore changes every frame. This isolates
// the row-incremental reshape (A2): only that one row's shaping/vertices
// should need regenerating once A2 lands.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use ratatui::layout::Rect as RatatuiRect;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;

use bevy_tui_texture::Font as TerminalFont;
use bevy_tui_texture::prelude::*;

const COLS: u16 = 120;
const ROWS: u16 = 40;

/// The one row that changes in `Mode::Partial`. Arbitrary - anywhere
/// inside the grid works, chosen away from the edges so it's easy to spot
/// on screen.
const COUNTER_ROW: u16 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Resource)]
enum Mode {
    Static,
    Partial,
}

impl Mode {
    fn from_env() -> Self {
        match std::env::var("BENCH_MODE").as_deref() {
            Ok("partial") => Mode::Partial,
            _ => Mode::Static,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Mode::Static => "static",
            Mode::Partial => "partial",
        }
    }
}

fn main() {
    let mode = Mode::from_env();
    println!("[benchmark_partial] mode={}", mode.label());

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("Terminal Benchmark Partial - Mode: {}", mode.label()),
                resolution: WindowResolution::new(1024, 768),
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(TerminalPlugin::display_only())
        .insert_resource(mode)
        .add_systems(Startup, setup)
        .add_systems(Update, render_benchmark_partial.in_set(TerminalSystemSet::Render))
        .run();
}

#[derive(Component)]
struct BenchmarkTerminal;

#[derive(Resource, Default)]
struct BenchmarkState {
    frame_count: u32,
    last_report_secs: f32,
}

fn setup(mut commands: Commands) {
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    commands.spawn((
        TuiRequest::ui(COLS, ROWS, fonts).with_config(TerminalConfig {
            keyboard: false,
            mouse: false,
            ..default()
        }),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(10.0),
            ..default()
        },
        BenchmarkTerminal,
    ));

    commands.spawn(Camera2d);
    commands.insert_resource(BenchmarkState::default());
}

fn render_benchmark_partial(
    mut screens: Query<&mut Tui, With<BenchmarkTerminal>>,
    mut state: ResMut<BenchmarkState>,
    mode: Res<Mode>,
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
) {
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    state.frame_count += 1;

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let elapsed = time.elapsed_secs();
    if elapsed - state.last_report_secs >= 2.0 {
        state.last_report_secs = elapsed;
        println!(
            "[benchmark:{}] t={elapsed:>6.1}s  fps={fps:>6.1}  frame_time_ms={:>6.2}  frames={}",
            mode.label(),
            if fps > 0.0 { 1000.0 / fps } else { 0.0 },
            state.frame_count
        );
    }

    let mode = *mode;
    let frame_count = state.frame_count;

    term.draw(|frame| {
        let area = frame.area();
        for y in 0..area.height {
            let line = if mode == Mode::Partial && y == COUNTER_ROW {
                counter_line(frame_count, area.width as usize)
            } else {
                static_pattern_line(y, area.width as usize)
            };

            frame.render_widget(
                Paragraph::new(line),
                RatatuiRect {
                    x: area.x,
                    y: area.y + y,
                    width: area.width,
                    height: 1,
                },
            );
        }
    });
}

/// Purely deterministic function of `(y, x)` only - no time, no frame
/// count - so the SAME row renders byte-identical content on every call.
/// This is what makes "static" mode's redraw a no-op past the first frame.
fn static_pattern_line(y: u16, width: usize) -> Line<'static> {
    let mut spans = Vec::with_capacity(width);
    for x in 0..width {
        let seed = (y as u32).wrapping_mul(73_856_093) ^ (x as u32).wrapping_mul(19_349_663);
        let hue = (seed % 360) as f32 / 360.0;
        let (r, g, b) = hsv_to_rgb(hue, 0.6, 0.8);
        spans.push(Span::styled(
            "█",
            Style::default().fg(RatatuiColor::Rgb(r, g, b)),
        ));
    }
    Line::from(spans)
}

/// The one line that changes every frame in `Mode::Partial`.
fn counter_line(frame_count: u32, width: usize) -> Line<'static> {
    let text = format!(" Frame: {frame_count:>10} - this row changes every frame ");
    let padded = format!("{text:<width$}", width = width);
    Line::from(Span::styled(
        padded,
        Style::default()
            .fg(RatatuiColor::Black)
            .bg(RatatuiColor::Yellow)
            .bold(),
    ))
}

// Uses Permuted Congruential Generator for better quality and speed -
// mirrors examples/benchmark.rs's hsv_to_rgb (kept identical so the two
// benchmarks' visual style matches).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let h_prime = h * 6.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = v - c;

    let (r, g, b) = match h_prime as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
