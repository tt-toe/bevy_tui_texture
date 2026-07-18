//! Lossy conversions between [`InputEvent`] and `crossterm::event::Event`.
//!
//! Native-only (`crossterm-compat` feature, off by default) - crossterm
//! does not build on wasm32-unknown-unknown. Both directions return `None`
//! for anything unrepresentable in the other vocabulary - not an error,
//! just "this event has no equivalent there". `Resize` never converts
//! either way: this crate's `Resize` carries a pixel size (texture
//! terminals don't auto-resize, so a cell count would be stale - see
//! `InputEvent::Resize`'s doc comment), while crossterm's carries the new
//! cell size; the units simply don't correspond.

use super::{InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use bevy::input::mouse::MouseButton;
use crossterm::event as ct;

impl InputEvent {
    /// `KeyCode::Unidentified`, non-Left/Right/Middle mouse buttons, and
    /// `Resize` return `None` - see the module doc comment.
    pub fn to_crossterm(&self) -> Option<ct::Event> {
        Some(match self {
            InputEvent::Key(k) => ct::Event::Key(key_to_crossterm(k)?),
            InputEvent::Mouse(m) => ct::Event::Mouse(mouse_to_crossterm(m)?),
            InputEvent::Paste(s) => ct::Event::Paste(s.clone()),
            InputEvent::FocusGained => ct::Event::FocusGained,
            InputEvent::FocusLost => ct::Event::FocusLost,
            InputEvent::Resize { .. } => return None,
        })
    }

    /// crossterm `KeyCode`s outside our mirror (media keys, keypad,
    /// modifier-only keys, ...), non-standard mouse buttons, and `Resize`
    /// return `None` - see the module doc comment.
    pub fn from_crossterm(event: &ct::Event) -> Option<InputEvent> {
        Some(match event {
            ct::Event::Key(k) => InputEvent::Key(key_from_crossterm(k)?),
            ct::Event::Mouse(m) => InputEvent::Mouse(mouse_from_crossterm(m)?),
            ct::Event::Paste(s) => InputEvent::Paste(s.clone()),
            ct::Event::FocusGained => InputEvent::FocusGained,
            ct::Event::FocusLost => InputEvent::FocusLost,
            ct::Event::Resize(..) => return None,
        })
    }
}

fn key_to_crossterm(k: &KeyEvent) -> Option<ct::KeyEvent> {
    let code = match k.code {
        KeyCode::Char(c) => ct::KeyCode::Char(c),
        KeyCode::Enter => ct::KeyCode::Enter,
        KeyCode::Tab => ct::KeyCode::Tab,
        KeyCode::BackTab => ct::KeyCode::BackTab,
        KeyCode::Backspace => ct::KeyCode::Backspace,
        KeyCode::Delete => ct::KeyCode::Delete,
        KeyCode::Insert => ct::KeyCode::Insert,
        KeyCode::Esc => ct::KeyCode::Esc,
        KeyCode::Left => ct::KeyCode::Left,
        KeyCode::Right => ct::KeyCode::Right,
        KeyCode::Up => ct::KeyCode::Up,
        KeyCode::Down => ct::KeyCode::Down,
        KeyCode::Home => ct::KeyCode::Home,
        KeyCode::End => ct::KeyCode::End,
        KeyCode::PageUp => ct::KeyCode::PageUp,
        KeyCode::PageDown => ct::KeyCode::PageDown,
        KeyCode::F(n) => ct::KeyCode::F(n),
        // Carries a bevy physical key with no crossterm equivalent - see
        // `KeyCode::Unidentified`'s doc comment.
        KeyCode::Unidentified(_) => return None,
    };
    Some(ct::KeyEvent::new_with_kind(
        code,
        modifiers_to_crossterm(k.modifiers),
        kind_to_crossterm(k.kind),
    ))
}

fn key_from_crossterm(k: &ct::KeyEvent) -> Option<KeyEvent> {
    let code = match k.code {
        ct::KeyCode::Char(c) => KeyCode::Char(c),
        ct::KeyCode::Enter => KeyCode::Enter,
        ct::KeyCode::Tab => KeyCode::Tab,
        ct::KeyCode::BackTab => KeyCode::BackTab,
        ct::KeyCode::Backspace => KeyCode::Backspace,
        ct::KeyCode::Delete => KeyCode::Delete,
        ct::KeyCode::Insert => KeyCode::Insert,
        ct::KeyCode::Esc => KeyCode::Esc,
        ct::KeyCode::Left => KeyCode::Left,
        ct::KeyCode::Right => KeyCode::Right,
        ct::KeyCode::Up => KeyCode::Up,
        ct::KeyCode::Down => KeyCode::Down,
        ct::KeyCode::Home => KeyCode::Home,
        ct::KeyCode::End => KeyCode::End,
        ct::KeyCode::PageUp => KeyCode::PageUp,
        ct::KeyCode::PageDown => KeyCode::PageDown,
        ct::KeyCode::F(n) => KeyCode::F(n),
        // Null/CapsLock/ScrollLock/NumLock/PrintScreen/Pause/Menu/
        // KeypadBegin/Media/Modifier: no bevy physical key to carry, so
        // (unlike the reverse direction) there is no `Unidentified`
        // payload to fill - just unrepresentable.
        _ => return None,
    };
    Some(KeyEvent {
        code,
        modifiers: modifiers_from_crossterm(k.modifiers),
        kind: kind_from_crossterm(k.kind),
    })
}

fn mouse_to_crossterm(m: &MouseEvent) -> Option<ct::MouseEvent> {
    Some(ct::MouseEvent {
        kind: mouse_kind_to_crossterm(m.kind)?,
        column: m.column,
        row: m.row,
        modifiers: modifiers_to_crossterm(m.modifiers),
    })
}

fn mouse_from_crossterm(m: &ct::MouseEvent) -> Option<MouseEvent> {
    Some(MouseEvent {
        kind: mouse_kind_from_crossterm(m.kind)?,
        column: m.column,
        row: m.row,
        modifiers: modifiers_from_crossterm(m.modifiers),
    })
}

fn mouse_kind_to_crossterm(kind: MouseEventKind) -> Option<ct::MouseEventKind> {
    Some(match kind {
        MouseEventKind::Down(b) => ct::MouseEventKind::Down(button_to_crossterm(b)?),
        MouseEventKind::Up(b) => ct::MouseEventKind::Up(button_to_crossterm(b)?),
        MouseEventKind::Drag(b) => ct::MouseEventKind::Drag(button_to_crossterm(b)?),
        MouseEventKind::Moved => ct::MouseEventKind::Moved,
        MouseEventKind::ScrollUp => ct::MouseEventKind::ScrollUp,
        MouseEventKind::ScrollDown => ct::MouseEventKind::ScrollDown,
        MouseEventKind::ScrollLeft => ct::MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight => ct::MouseEventKind::ScrollRight,
    })
}

fn mouse_kind_from_crossterm(kind: ct::MouseEventKind) -> Option<MouseEventKind> {
    Some(match kind {
        ct::MouseEventKind::Down(b) => MouseEventKind::Down(button_from_crossterm(b)?),
        ct::MouseEventKind::Up(b) => MouseEventKind::Up(button_from_crossterm(b)?),
        ct::MouseEventKind::Drag(b) => MouseEventKind::Drag(button_from_crossterm(b)?),
        ct::MouseEventKind::Moved => MouseEventKind::Moved,
        ct::MouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
        ct::MouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
        ct::MouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
        ct::MouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
    })
}

fn button_to_crossterm(b: MouseButton) -> Option<ct::MouseButton> {
    Some(match b {
        MouseButton::Left => ct::MouseButton::Left,
        MouseButton::Right => ct::MouseButton::Right,
        MouseButton::Middle => ct::MouseButton::Middle,
        _ => return None,
    })
}

fn button_from_crossterm(b: ct::MouseButton) -> Option<MouseButton> {
    Some(match b {
        ct::MouseButton::Left => MouseButton::Left,
        ct::MouseButton::Right => MouseButton::Right,
        ct::MouseButton::Middle => MouseButton::Middle,
    })
}

fn kind_to_crossterm(kind: KeyEventKind) -> ct::KeyEventKind {
    match kind {
        KeyEventKind::Press => ct::KeyEventKind::Press,
        KeyEventKind::Repeat => ct::KeyEventKind::Repeat,
        KeyEventKind::Release => ct::KeyEventKind::Release,
    }
}

fn kind_from_crossterm(kind: ct::KeyEventKind) -> KeyEventKind {
    match kind {
        ct::KeyEventKind::Press => KeyEventKind::Press,
        ct::KeyEventKind::Repeat => KeyEventKind::Repeat,
        ct::KeyEventKind::Release => KeyEventKind::Release,
    }
}

fn modifiers_to_crossterm(m: KeyModifiers) -> ct::KeyModifiers {
    let mut out = ct::KeyModifiers::NONE;
    out.set(ct::KeyModifiers::SHIFT, m.shift);
    out.set(ct::KeyModifiers::CONTROL, m.ctrl);
    out.set(ct::KeyModifiers::ALT, m.alt);
    out.set(ct::KeyModifiers::SUPER, m.meta);
    out
}

fn modifiers_from_crossterm(m: ct::KeyModifiers) -> KeyModifiers {
    KeyModifiers {
        ctrl: m.contains(ct::KeyModifiers::CONTROL),
        alt: m.contains(ct::KeyModifiers::ALT),
        shift: m.contains(ct::KeyModifiers::SHIFT),
        meta: m.contains(ct::KeyModifiers::SUPER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::KeyCode as BevyKeyCode;

    fn assert_round_trip(event: InputEvent) {
        let ct_event = event.to_crossterm().expect("representable event");
        let back = InputEvent::from_crossterm(&ct_event).expect("representable event");
        assert_eq!(event, back);
    }

    #[test]
    fn key_char_round_trips() {
        assert_round_trip(InputEvent::Key(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
            kind: KeyEventKind::Press,
        }));
    }

    #[test]
    fn key_backtab_round_trips() {
        assert_round_trip(InputEvent::Key(KeyEvent {
            code: KeyCode::BackTab,
            modifiers: KeyModifiers {
                shift: true,
                ..Default::default()
            },
            kind: KeyEventKind::Release,
        }));
    }

    #[test]
    fn mouse_scroll_round_trips() {
        assert_round_trip(InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 10,
            modifiers: KeyModifiers::default(),
        }));
    }

    #[test]
    fn unidentified_key_has_no_crossterm_equivalent() {
        let event = InputEvent::Key(KeyEvent {
            code: KeyCode::Unidentified(BevyKeyCode::AltLeft),
            modifiers: KeyModifiers::default(),
            kind: KeyEventKind::Press,
        });
        assert_eq!(event.to_crossterm(), None);
    }

    #[test]
    fn resize_has_no_crossterm_equivalent_either_direction() {
        let event = InputEvent::Resize {
            pixels: bevy::math::UVec2::new(800, 600),
        };
        assert_eq!(event.to_crossterm(), None);
        assert_eq!(
            InputEvent::from_crossterm(&ct::Event::Resize(80, 24)),
            None
        );
    }
}
