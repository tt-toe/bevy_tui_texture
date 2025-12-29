// Benchmark Example - Terminal Drawing Performance Test
//
// Tests rendering performance with:
// - Mode 1: Full-screen scrolling color gradation
// - Mode 2: Random overlapping colored boxes

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
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

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Terminal Benchmark - No FPS Limit".to_string(),
                resolution: WindowResolution::new(1024, 768),
                present_mode: PresentMode::AutoNoVsync, // No FPS limit
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(TerminalPlugin::display_only()) // No input systems!
        .add_systems(Startup, setup)
        // NO INPUT AT ALL - pure rendering benchmark
        .add_systems(Update, render_benchmark.in_set(TerminalSystemSet::Render))
        .run();
}

#[derive(Resource)]
struct BenchmarkTerminal {
    terminal: SimpleTerminal2D,
}

#[derive(Resource, Default)]
struct BenchmarkState {
    mode: u8, // 0 = gradient, 1 = random boxes
    scroll_offset: f32,
    frame_count: u32,
}

fn setup(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let font = TerminalFont::new(font_data).expect("Failed to load font");
    let fonts = Arc::new(Fonts::new(font, 16));

    let terminal = SimpleTerminal2D::create_and_spawn(
        COLS,
        ROWS,
        fonts,
        (10.0, 10.0),
        true,
        false,
        false, // NO INPUT - pure rendering benchmark
        &mut commands,
        &render_device,
        &render_queue,
        &mut images,
    )
    .expect("Failed to create terminal");

    commands.spawn(Camera2d);
    commands.insert_resource(BenchmarkTerminal { terminal });
    commands.insert_resource(BenchmarkState::default());
}

fn render_benchmark(
    mut terminal_res: ResMut<BenchmarkTerminal>,
    mut state: ResMut<BenchmarkState>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
) {
    state.frame_count += 1;
    state.scroll_offset += time.delta_secs() * 20.0;

    // Auto-switch modes every 5 seconds (no input needed)
    state.mode = ((time.elapsed_secs() / 5.0) as u8) % 2;

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    // Get time in milliseconds for better random seed
    let time_ms = (time.elapsed_secs() * 1000.0) as u32;

    terminal_res
        .terminal
        .draw_and_render(&render_device, &render_queue, &mut images, |frame| {
            let area = frame.area();

            // Mode display
            let mode_name = match state.mode {
                0 => "Mode 1: Scrolling Gradation",
                1 => "Mode 2: Random Boxes",
                _ => unreachable!(),
            };

            let info = format!(
                "FPS: {:>5.1} | Frames: {:>6} | {} | [NO INPUT - Auto-switching every 5s]",
                fps, state.frame_count, mode_name
            );

            let header = Paragraph::new(info)
                .style(Style::default().fg(RatatuiColor::Yellow).bold())
                .block(Block::bordered());

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)])
                .split(area);

            frame.render_widget(header, chunks[0]);

            // Render content based on mode
            match state.mode {
                0 => render_gradient(frame, chunks[1], state.scroll_offset),
                1 => render_random_boxes(frame, chunks[1], time_ms + state.frame_count),
                _ => unreachable!(),
            }
        });
}

fn render_gradient(frame: &mut ratatui::Frame, area: RatatuiRect, offset: f32) {
    let width = area.width as usize;
    let height = area.height as usize;

    for y in 0..height {
        let mut line_spans = Vec::new();

        for x in 0..width {
            // Calculate color based on position + scroll offset
            let hue = ((x as f32 + y as f32 * 3.0 + offset) % 360.0) / 360.0;
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);

            line_spans.push(Span::styled(
                "█",
                Style::default().fg(RatatuiColor::Rgb(r, g, b)),
            ));
        }

        let line = Line::from(line_spans);
        let x = area.x;
        let y_pos = area.y + y as u16;

        if y_pos < area.y + area.height {
            frame.render_widget(
                Paragraph::new(line),
                RatatuiRect {
                    x,
                    y: y_pos,
                    width: area.width,
                    height: 1,
                },
            );
        }
    }
}

fn render_random_boxes(frame: &mut ratatui::Frame, area: RatatuiRect, seed: u32) {
    // Draw 50 random boxes per frame
    for i in 0..50 {
        let x = pseudo_random(seed.wrapping_add(i * 4)) % area.width;
        let y = pseudo_random(seed.wrapping_add(i * 4 + 1)) % area.height;
        let w = (pseudo_random(seed.wrapping_add(i * 4 + 2)) % 20).max(5);
        let h = (pseudo_random(seed.wrapping_add(i * 4 + 3)) % 10).max(3);

        let hue = (pseudo_random(seed.wrapping_add(i)) % 360) as f32 / 360.0;
        let (r, g, b) = hsv_to_rgb(hue, 0.8, 0.9);

        let box_rect = RatatuiRect {
            x: area.x + x.min(area.width.saturating_sub(w)),
            y: area.y + y.min(area.height.saturating_sub(h)),
            width: w.min(area.width),
            height: h.min(area.height),
        };

        let block = Block::bordered().style(Style::default().fg(RatatuiColor::Rgb(r, g, b)));

        frame.render_widget(block, box_rect);
    }
}

// Uses Permuted Congruential Generator for better quality and speed
fn pseudo_random(seed: u32) -> u16 {
    // PCG XSH RR 32/16 variant
    let state = seed.wrapping_mul(747796405u32).wrapping_add(2891336453u32);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277803737u32);
    ((word >> 22) ^ word) as u16
}

// Convert HSV to RGB
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
