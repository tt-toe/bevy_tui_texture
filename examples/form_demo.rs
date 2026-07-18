//! form_demo - a complete interactive form built on nothing but the
//! low-level event contract (`TerminalEvent` + `HitRegions`).
//!
//! This crate deliberately ships no form/widget framework; this example
//! is the reference for how little app code that decision costs. Widget
//! identity, focus traversal, and value state are plain Rust:
//! - Click a field to focus it; click Subscribe/Submit/Clear to activate.
//! - Tab / Shift+Tab cycle widget focus; Enter activates the focused one.
//! - Type / Backspace edit the Name field. Esc clears the form.
//! - The scroll wheel (or Enter/click) cycles the Plan field.

use bevy::prelude::*;
use bevy_tui_texture::Font as TerminalFont;
// Explicit import: shadows bevy::prelude::KeyCode (globs never override an
// explicit `use`), so `KeyCode::Tab` below means the terminal key mirror.
use bevy_tui_texture::input::KeyCode;
use bevy_tui_texture::prelude::*;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Paragraph};
use std::sync::Arc;

#[derive(Component)]
struct FormTerminal;

/// Widget identity: a plain enum encoded into the `u64` ids that
/// `HitRegions` already speaks. `From`/`TryFrom` are the entire
/// "framework integration".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Widget {
    Name,
    Plan,
    Subscribe,
    Submit,
    Clear,
}

const TAB_ORDER: [Widget; 5] = [
    Widget::Name,
    Widget::Plan,
    Widget::Subscribe,
    Widget::Submit,
    Widget::Clear,
];

impl From<Widget> for u64 {
    fn from(w: Widget) -> u64 {
        w as u64
    }
}

impl TryFrom<u64> for Widget {
    type Error = ();
    fn try_from(v: u64) -> Result<Self, ()> {
        TAB_ORDER.iter().copied().find(|w| *w as u64 == v).ok_or(())
    }
}

const PLANS: [&str; 3] = ["Free", "Pro", "Team"];

#[derive(Resource)]
struct FormModel {
    focused: Widget,
    name: String,
    plan: usize,
    subscribe: bool,
    status: String,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(TerminalPlugin::default())
        .insert_resource(FormModel {
            focused: Widget::Name,
            name: String::new(),
            plan: 0,
            subscribe: false,
            status: "click or Tab to focus, Enter to activate".into(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, handle_events.in_set(TerminalSystemSet::UserUpdate))
        .add_systems(Update, render_form.in_set(TerminalSystemSet::Render))
        .run();
}

fn setup(mut commands: Commands) {
    let font_data = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/assets/fonts/Mplus1Code-Regular.ttf"
    ));
    let fonts = Arc::new(Fonts::new(
        TerminalFont::new(font_data).expect("failed to parse font"),
        16,
    ));

    commands.spawn(Camera2d);
    let terminal = commands
        .spawn((TuiRequest::ui(60, 18, fonts), Node::default(), FormTerminal))
        .id();
    commands.insert_resource(TerminalFocus {
        focused: Some(terminal),
    });
}

fn handle_events(
    mut events: MessageReader<TerminalEvent>,
    mut form: ResMut<FormModel>,
    terminals: Query<(Entity, &Tui), With<FormTerminal>>,
) {
    let Ok((entity, term)) = terminals.single() else {
        return;
    };
    for event in events.read().filter(|e| e.target == entity) {
        match &event.input {
            InputEvent::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(w) = term.hit_regions().hit_at::<Widget>((m.column, m.row)) {
                        form.focused = w;
                        activate(&mut form, w);
                    }
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    if term.hit_regions().hit_at::<Widget>((m.column, m.row))
                        == Some(Widget::Plan) =>
                {
                    let step = if m.kind == MouseEventKind::ScrollUp {
                        PLANS.len() - 1 // backwards, wrapping
                    } else {
                        1
                    };
                    form.plan = (form.plan + step) % PLANS.len();
                }
                _ => {}
            },
            InputEvent::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Tab => shift_focus(&mut form, 1),
                KeyCode::BackTab => shift_focus(&mut form, TAB_ORDER.len() - 1),
                KeyCode::Enter => {
                    let w = form.focused;
                    activate(&mut form, w);
                }
                KeyCode::Esc => clear(&mut form),
                KeyCode::Char(c) if form.focused == Widget::Name => form.name.push(c),
                KeyCode::Backspace if form.focused == Widget::Name => {
                    form.name.pop();
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn shift_focus(form: &mut FormModel, by: usize) {
    let i = TAB_ORDER
        .iter()
        .position(|w| *w == form.focused)
        .unwrap_or(0);
    form.focused = TAB_ORDER[(i + by) % TAB_ORDER.len()];
}

fn activate(form: &mut FormModel, widget: Widget) {
    match widget {
        Widget::Name => {} // focus only; typing edits it
        Widget::Plan => form.plan = (form.plan + 1) % PLANS.len(),
        Widget::Subscribe => form.subscribe = !form.subscribe,
        Widget::Submit => {
            form.status = format!(
                "submitted: name={:?} plan={} subscribe={}",
                form.name, PLANS[form.plan], form.subscribe
            );
        }
        Widget::Clear => clear(form),
    }
}

fn clear(form: &mut FormModel) {
    form.name.clear();
    form.plan = 0;
    form.subscribe = false;
    form.status = "cleared".into();
}

fn render_form(mut terminals: Query<&mut Tui, With<FormTerminal>>, form: Res<FormModel>) {
    let Ok(mut term) = terminals.single_mut() else {
        return;
    };
    let focused = form.focused;
    term.draw_with_hits(|frame, hits| {
        let rows = Layout::vertical([
            Constraint::Length(3), // Name
            Constraint::Length(3), // Plan
            Constraint::Length(3), // Subscribe
            Constraint::Length(3), // buttons
            Constraint::Min(1),    // status line
        ])
        .split(frame.area());

        let field = |title: &'static str, w: Widget| {
            let style = if focused == w {
                Style::new().yellow().bold()
            } else {
                Style::new()
            };
            Block::bordered().title(title).border_style(style)
        };

        frame.render_widget(
            Paragraph::new(form.name.as_str()).block(field("Name", Widget::Name)),
            rows[0],
        );
        hits.add(Widget::Name, rows[0]);

        frame.render_widget(
            Paragraph::new(format!("< {} >  (Enter or wheel)", PLANS[form.plan]))
                .block(field("Plan", Widget::Plan)),
            rows[1],
        );
        hits.add(Widget::Plan, rows[1]);

        let checkbox = if form.subscribe { "[x]" } else { "[ ]" };
        frame.render_widget(
            Paragraph::new(format!("{checkbox} subscribe to newsletter"))
                .block(field("Subscribe", Widget::Subscribe)),
            rows[2],
        );
        hits.add(Widget::Subscribe, rows[2]);

        let buttons = Layout::horizontal([
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Min(0),
        ])
        .split(rows[3]);
        frame.render_widget(
            Paragraph::new("Submit").centered().block(field("", Widget::Submit)),
            buttons[0],
        );
        hits.add(Widget::Submit, buttons[0]);
        frame.render_widget(
            Paragraph::new("Clear").centered().block(field("", Widget::Clear)),
            buttons[1],
        );
        hits.add(Widget::Clear, buttons[1]);

        frame.render_widget(Paragraph::new(form.status.as_str()).dim(), rows[4]);
    });
}
