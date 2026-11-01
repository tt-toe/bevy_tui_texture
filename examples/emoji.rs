//! Color emoji.

use bevy::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
use bevy_tui_texture::prelude::*;
use font_kit::family_name::FamilyName;
use font_kit::properties::Properties;
use font_kit::source::SystemSource;
use ratatui::prelude::*;
use ratatui::style::Color as RatatuiColor;
use ratatui::widgets::*;
use std::sync::Arc;

const COLS: u16 = 46;
const ROWS: u16 = 18;
const FONT_SIZE: u32 = 28;

/// Where the emoji font came from, shown in the title.
#[derive(Resource)]
struct EmojiFontName(Option<String>);

#[derive(Component)]
struct EmojiTerminal;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Color emoji".to_string(),
                resolution: (1100, 780).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
        .run();
}

/// The emoji font: `$EMOJI_FONT`, else an installed color emoji font.
fn load_emoji_font() -> Option<(String, Vec<u8>)> {
    if let Ok(path) = std::env::var("EMOJI_FONT") {
        match std::fs::read(&path) {
            Ok(bytes) => return Some((path, bytes)),
            Err(err) => warn!("EMOJI_FONT={path}: {err}"),
        }
    }
    let source = SystemSource::new();
    ["Noto Color Emoji", "Apple Color Emoji"]
        .into_iter()
        .find_map(|family| {
            let handle = source
                .select_best_match(&[FamilyName::Title(family.into())], &Properties::new())
                .ok()?;
            let font = handle.load().ok()?;
            (font.family_name() == family)
                .then(|| Some((family.to_string(), font.copy_font_data()?.to_vec())))
                .flatten()
        })
}

fn setup(mut commands: Commands) {
    let emoji_font = load_emoji_font();
    if emoji_font.is_none() {
        warn!(
            "No emoji font found: set EMOJI_FONT=/path/to/NotoColorEmoji.ttf. \
             Emoji will show as missing glyphs."
        );
    }

    let mut fonts = Fonts::new(
        TerminalFont::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/assets/fonts/Mplus1Code-Regular.ttf"
        )))
        .expect("M+ parses"),
        FONT_SIZE,
    );
    // Emoji fonts never change the cell size, whatever their metrics.
    fonts.add_emoji_fonts(
        emoji_font
            .as_ref()
            .and_then(|(_, bytes)| TerminalFont::from_vec(bytes.clone())),
    );

    commands.insert_resource(EmojiFontName(emoji_font.map(|(name, _)| name)));
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|root| {
            root.spawn((
                TuiRequest::ui(COLS, ROWS, Arc::new(fonts)).with_config(TerminalConfig {
                    keyboard: false,
                    mouse: false,
                    ..default()
                }),
                Node::default(),
                EmojiTerminal,
            ));
        });
}

/// One line: `(text, style)` spans.
fn line(spans: &[(&str, Style)]) -> Line<'static> {
    Line::from(
        spans
            .iter()
            .map(|&(text, style)| Span::styled(text.to_string(), style))
            .collect::<Vec<_>>(),
    )
}

fn render_terminal(mut screens: Query<&mut Tui, With<EmojiTerminal>>, font: Res<EmojiFontName>) {
    use RatatuiColor::*;
    let Ok(mut term) = screens.single_mut() else {
        return;
    };
    let source = match &font.0 {
        Some(name) => name.rsplit('/').next().unwrap_or(name).to_string(),
        None => "no emoji font found - set EMOJI_FONT".to_string(),
    };
    term.draw(|frame| {
        let block = Block::bordered()
            .title(" Color emoji ")
            .title_bottom(Line::from(format!(" {source} ")).right_aligned())
            .border_style(Style::default().fg(DarkGray));
        let area = block.inner(frame.area());
        frame.render_widget(block, frame.area());

        let label = Style::default().fg(Gray);
        let plain = Style::default();
        let on_blue = Style::default().fg(White).bg(Rgb(30, 60, 140));
        let on_green = Style::default().bg(Rgb(20, 90, 50));
        let lines = vec![
            line(&[("Single emoji, two cells each", label)]),
            line(&[("\u{1F600}\u{1F680}\u{1F980}\u{1F525}\u{1F389}\u{1F355}", plain)]),
            Line::default(),
            line(&[("Sequences: skin tone, ZWJ, flag, keycap", label)]),
            line(&[(
                "\u{1F44D}\u{1F3FD} \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{1F1EF}\u{1F1F5} 1\u{FE0F}\u{20E3} \u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
                plain,
            )]),
            Line::default(),
            line(&[("Presentation: \u{2764}\u{FE0F} emoji, \u{2764} text", label)]),
            line(&[
                ("\u{2764}\u{FE0F}", Style::default().fg(Red)),
                (" in the text color: ", plain),
                ("\u{2764}", Style::default().fg(Red)),
            ]),
            Line::default(),
            line(&[("Over a background, next to text", label)]),
            line(&[
                (" a\u{1F600}b ", on_blue),
                (" ", plain),
                ("\u{1F680}", on_green),
                (" launch ", on_green),
            ]),
            Line::default(),
            line(&[("A combining mark stays in the text color", label)]),
            line(&[
                ("\u{1F600}\u{301}", Style::default().fg(Magenta)),
                (" \u{1F600}", plain),
            ]),
            Line::default(),
            line(&[(
                "Bold text, emoji not smeared \u{1F980}",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
        ];
        frame.render_widget(Paragraph::new(lines), area);
    });
}
