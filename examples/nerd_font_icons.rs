//! Nerd Font icons fitted to the cell grid.
//!
//! Two terminals show the same screen - a file tree, a status line and a
//! few edge cases - with symbol fitting on (left, the default) and off
//! (right, `Fonts::set_fit_symbols(false)`). On the left an icon followed
//! by a space spreads over both cells at its designed aspect ratio; on the
//! right it is squeezed into one cell.
//!
//! No Nerd Font ships with this crate, so the icons come from a font loaded
//! at startup:
//! - `NERD_FONT=/path/to/SymbolsNerdFontMono-Regular.ttf` (any Nerd Font
//!   `.ttf`), or else
//! - an installed "Symbols Nerd Font Mono" / "Symbols Nerd Font".
//!
//! Prefer the Mono symbols font as a fallback: the non-Mono one has no `m`,
//! and its narrower `.notdef` advance shrinks every cell.
//! fusion-pixel (shipped) is the last fallback, for the full-width arrows
//! and stars on the last lines, which are fitted the same way.
//!
//! Run with: `NERD_FONT=... cargo run --example nerd_font_icons`

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

const COLS: u16 = 44;
const ROWS: u16 = 22;
const FONT_SIZE: u32 = 22;

/// One of the two terminals; `true` when symbol fitting is on.
#[derive(Component)]
struct IconTerminal(bool);

/// Where the icons came from, shown in the title.
#[derive(Resource)]
struct IconFontName(Option<String>);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Nerd Font icons - fitted (left) vs. squeezed (right)".to_string(),
                resolution: (1100, 560).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, render_terminals.in_set(TerminalSystemSet::Render))
        .run();
}

/// The icon font: `$NERD_FONT`, else an installed Symbols Nerd Font.
fn load_icon_font() -> Option<(String, Vec<u8>)> {
    if let Ok(path) = std::env::var("NERD_FONT") {
        match std::fs::read(&path) {
            Ok(bytes) => return Some((path, bytes)),
            Err(err) => warn!("NERD_FONT={path}: {err}"),
        }
    }
    let source = SystemSource::new();
    ["Symbols Nerd Font Mono", "Symbols Nerd Font"]
        .into_iter()
        .find_map(|family| {
            let handle = source
                .select_best_match(&[FamilyName::Title(family.into())], &Properties::new())
                .ok()?;
            let font = handle.load().ok()?;
            // `select_best_match` falls back to any font; keep only a match.
            (font.family_name() == family)
                .then(|| Some((family.to_string(), font.copy_font_data()?.to_vec())))
                .flatten()
        })
}

fn setup(mut commands: Commands) {
    let icon_font = load_icon_font();
    if icon_font.is_none() {
        warn!(
            "No Nerd Font found: set NERD_FONT=/path/to/SymbolsNerdFontMono-Regular.ttf \
             or install Symbols Nerd Font. Icons will show as missing glyphs."
        );
    }

    let fonts = |fit_symbols: bool| {
        // Tried in order: M+ for text, the icon font, fusion-pixel last
        // (the last-resort slot).
        let mut fonts = Fonts::new(
            TerminalFont::new(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf"
            )))
            .expect("fusion-pixel parses"),
            FONT_SIZE,
        );
        let mplus = TerminalFont::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/assets/fonts/Mplus1Code-Regular.ttf"
        )))
        .expect("M+ parses");
        let icons = icon_font
            .as_ref()
            .and_then(|(_, bytes)| TerminalFont::from_vec(bytes.clone()));
        fonts.add_regular_fonts(std::iter::once(mplus).chain(icons));
        fonts.set_fit_symbols(fit_symbols);
        Arc::new(fonts)
    };

    // Two `Fonts` (fitting on / off), so two separate glyph atlases.
    let (fitted, squeezed) = (fonts(true), fonts(false));
    commands.insert_resource(IconFontName(icon_font.map(|(name, _)| name)));
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(|row| {
            for (fit_symbols, fonts) in [(true, fitted), (false, squeezed)] {
                row.spawn((
                    TuiRequest::ui(COLS, ROWS, fonts).with_config(TerminalConfig {
                        keyboard: false,
                        mouse: false,
                        ..default()
                    }),
                    Node::default(),
                    IconTerminal(fit_symbols),
                ));
            }
        });
}

/// One line: `(text, color)` spans.
fn line(spans: &[(&str, RatatuiColor)]) -> Line<'static> {
    Line::from(
        spans
            .iter()
            .map(|&(text, color)| Span::styled(text.to_string(), Style::default().fg(color)))
            .collect::<Vec<_>>(),
    )
}

fn render_terminals(mut screens: Query<(&mut Tui, &IconTerminal)>, icon_font: Res<IconFontName>) {
    use RatatuiColor::*;
    for (mut term, IconTerminal(fit_symbols)) in &mut screens {
        let title = format!(" set_fit_symbols({fit_symbols}) ");
        let source = match &icon_font.0 {
            Some(name) => name.rsplit('/').next().unwrap_or(name).to_string(),
            None => "no Nerd Font found - set NERD_FONT".to_string(),
        };
        term.draw(|frame| {
            let block = Block::bordered()
                .title(title.as_str())
                .title_bottom(Line::from(format!(" {source} ")).right_aligned())
                .border_style(Style::default().fg(DarkGray));
            let area = block.inner(frame.area());
            frame.render_widget(block, frame.area());

            let bar = Style::default().bg(Rgb(40, 44, 52));
            let lines = vec![
                line(&[("File tree (icon + space + name)", Gray)]),
                line(&[("\u{F07C} ", Yellow), ("src/", Blue)]),
                line(&[("  \u{E7A8} ", LightRed), ("main.rs", White)]),
                line(&[("  \u{E7A8} ", LightRed), ("lib.rs", White)]),
                line(&[("  \u{F07B} ", Yellow), ("backend/", Blue)]),
                line(&[("\u{E615} ", Gray), ("Cargo.toml", White)]),
                line(&[("\u{F48A} ", LightBlue), ("README.md", White)]),
                line(&[("\u{E702} ", LightRed), (".gitignore", DarkGray)]),
                Line::default(),
                line(&[("Status line (Powerline + icons)", Gray)]),
                Line::from(vec![
                    Span::styled(" \u{E0A0} main ", Style::default().fg(Black).bg(Green)),
                    Span::styled("\u{E0B0}", Style::default().fg(Green).bg(Rgb(40, 44, 52))),
                    Span::styled(" \u{F0E7} 3 ", bar.fg(Yellow)),
                    Span::styled("\u{F071} 1 ", bar.fg(LightRed)),
                    Span::styled("\u{E0B0}", Style::default().fg(Rgb(40, 44, 52))),
                ]),
                Line::default(),
                line(&[("No space after: one cell each", Gray)]),
                line(&[
                    ("\u{F07B}\u{F07C}\u{F015}\u{F013}\u{F0E7}\u{F071}", Cyan),
                    ("|end", DarkGray),
                ]),
                line(&[("A space after each: two cells", Gray)]),
                line(&[
                    (
                        "\u{F07B} \u{F07C} \u{F015} \u{F013} \u{F0E7} \u{F071} ",
                        Cyan,
                    ),
                    ("|end", DarkGray),
                ]),
                Line::default(),
                line(&[("Full-width symbols (fusion-pixel)", Gray)]),
                line(&[("→ next  ★ star  ○ ring  ※ note", Magenta)]),
                line(&[("→next ★star ○ring ※note", Magenta)]),
            ];
            frame.render_widget(Paragraph::new(lines), area);
        });
    }
}
