# EVENTS.md — Input Event Model Redesign (implementation spec)

Target release: **0.4** (breaking). Audience: an implementing agent — every
type, rule, and file change is spelled out; where a bevy/crossterm detail
must be verified against the actual dependency source, the spec says so
explicitly.

## 0. Goal and guiding principles

Make `bevy_tui_texture` feel like **bevy_ratatui** to its users:

> Events arrive through Bevy's message system in **crossterm-shaped
> vocabulary**; drawing is plain ratatui. The crate is a transport +
> rendering layer, **not** a UI framework.

Principles (in priority order):

1. **Stop at the event vocabulary.** No forms, no widgets, no validation
   in this crate — ever. Those belong to app code or a future separate
   crate (see §8). The `form_demo` example (§6) proves the low-level
   contract is sufficient by building a complete form in ~180 lines of
   plain app code.
2. **Minimal source footprint.** Every addition must be offset where
   possible by a deletion (the hand-written US-layout key table dies).
   Budget: net diff in `src/input/` ≤ +150 lines; `crossterm-compat`
   module ≤ 120 lines; no new modules besides that one.
3. **Envelope is Bevy-native, payload is crossterm-shaped.** Routing
   (`target: Entity`) is a Bevy concept; the payload mirrors
   `crossterm::event::Event` so the ratatui ecosystem's input vocabulary
   (tui-textarea, bevy_ratatui, etc.) maps 1:1.
4. **No unconditional crossterm dependency.** crossterm does not build on
   wasm32-unknown-unknown, and wasm is a first-class target here. The
   payload types are self-defined mirrors; real crossterm conversion is an
   opt-in, native-only feature.

### Why crossterm-shaped? (survey summary, July 2026)

- [bevy_ratatui](https://github.com/ratatui/bevy_ratatui) (the only proven
  terminal-input→Bevy bridge) re-emits crossterm events as Bevy messages
  (`KeyMessage` etc.) and optionally forwards them into native Bevy input
  (`translation` module, with terminal-capability detection and key-release
  emulation). Its users think in crossterm vocabulary.
- egui_ratatui leaves input to the host; users conventionally translate to
  crossterm-shaped events because ratatui widget crates (tui-textarea's
  `Input: From<crossterm::event::Event>`, etc.) consume that shape.
- Conclusion: crossterm's event shape is the ratatui ecosystem's lingua
  franca. Mirroring it (not depending on it) maximizes compatibility while
  keeping wasm intact.

## 1. Scope

**In scope (this spec):**

- S1. Replace `TerminalEventType` with crossterm-shaped `InputEvent`
  (§3.1) and rewrite the capture systems accordingly (§3.2–3.5).
- S2. Opt-in `crossterm-compat` feature: lossy conversions between
  `InputEvent` and `crossterm::event::Event` (§3.7).
- S3. Decouple `HitRegions` from `Tui` just enough for display-agnostic
  reuse: `#[derive(Component)]` + `pub fn clear` (§3.6). Two lines.
- S4. New example `examples/form_demo.rs` (§6) + migration of existing
  examples (§5).

**Explicitly out of scope (§8):** form/validation framework, TOML form
definition, widget library, bevy_ratatui adapter crate, IME composition
events, deprecation shims for the old enum (clean break at 0.4).

## 2. Current state and defects being fixed

Current model (`src/input/mod.rs`): capture systems read
`ButtonInput<KeyCode>` / `Touches` / cursor position, hit-test (2D UI
bounds or 3D ray-mesh UV), and emit
`TerminalEvent { target: Entity, event: TerminalEventType }`.

| # | Defect | Fix |
|---|--------|-----|
| C1 | `KeyPress` (physical bevy `KeyCode`) and `CharInput` are two separate events per keystroke; ratatui-ecosystem consumers expect one crossterm-style `KeyEvent` | Single `InputEvent::Key` with `KeyCode::Char(c)` (§3.1) |
| C2 | `keycode_to_char` is a ~60-line hand-written **US-layout-only** table | Delete it; derive characters from `KeyboardInput::logical_key`, which winit already resolves per layout (§3.2) |
| C3 | No scroll-wheel, no drag, no paste vocabulary | `MouseEventKind::{Scroll*, Drag}`, `InputEvent::Paste` (§3.1, §3.3) |
| C4 | `Resize` carried an anonymous `(u32, u32)` tuple | `Resize { pixels: UVec2 }` — pixels stay the payload **on purpose**: texture terminals never auto-resize, so a crossterm-style "new cols/rows" field would report a stale/unchanged grid and silently mislead crossterm-habituated readers. Cell-shape parity is claimed for Key/Mouse only (§3.4) |
| C5 | `TerminalEventType::MousePress.position` is documented "(row, col)" but actually emitted as `(col, row)` | Named fields `column`/`row` kill the ambiguity |

What is deliberately **kept unchanged**: the entity-targeted envelope,
`TuiSurface` remapping, all hit-testing machinery (2D bounds, 3D ray-mesh,
multi-camera priority, DPI handling), touch→mouse normalization (crossterm
has no touch vocabulary, so "touch = mouse" is also the correct
*compatibility* answer), `TerminalFocus` + Tab cycling,
`TerminalInputConfig`, and the `HitRegions` u64 registry API.

## 3. Specification

### 3.1 New event types (`src/input/mod.rs`)

Replace `TerminalEventType` and its helper types with:

```rust
/// Entity-targeted input message. The envelope is Bevy-native; the
/// payload mirrors `crossterm::event::Event` (see module docs for why).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct TerminalEvent {
    /// The Tui entity that should receive this message.
    pub target: Entity,
    pub input: InputEvent,
}

/// Mirror of `crossterm::event::Event`. Self-defined because crossterm
/// does not build on wasm32; see the `crossterm-compat` feature for real
/// conversions on native.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// Never produced by the built-in capture systems (winit has no paste
    /// event). Exists for crossterm-shape parity so BYO writers (e.g. a
    /// bevy_ratatui adapter, tests, network input) can deliver it.
    Paste(String),
    FocusGained,
    FocusLost,
    /// Emitted on window resize for every terminal (same trigger as
    /// today). Deliberately NOT crossterm's `Resize(cols, rows)`: texture
    /// terminals never auto-resize, so a cell-count field here would
    /// report the old, unchanged grid and mislead crossterm-habituated
    /// readers. The pixel size is what `Tui::request_resize` recipes
    /// actually need (see examples/resize.rs); grid changes are always
    /// caller-initiated, so the caller already knows them.
    Resize { pixels: UVec2 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

/// Mirror of `crossterm::event::KeyCode` (the subset a GPU-windowed app
/// can produce), plus an escape hatch for everything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    /// Shift+Tab, following crossterm's convention (modifiers.shift stays
    /// true as well). Known, accepted quirk: derived from the *live* shift
    /// state at each key transition, so releasing Shift before releasing
    /// Tab yields a `Press` of `BackTab` but a `Release` of `Tab`. Do not
    /// try to fix this with per-key state tracking — crossterm has the
    /// same ambiguity, and the standard consumer pattern (ignore
    /// `Release` events) never observes it.
    BackTab,
    Backspace,
    Delete,
    Insert,
    Esc,
    Left, Right, Up, Down,
    Home, End, PageUp, PageDown,
    F(u8),
    /// Anything not representable above — carries the physical key so no
    /// information is lost. NOT part of the crossterm mirror; converts to
    /// `None` in `crossterm-compat`.
    Unidentified(bevy::input::keyboard::KeyCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    /// Terminal grid coordinates (cell units), like crossterm.
    pub column: u16,
    pub row: u16,
    pub modifiers: KeyModifiers,
}

/// Mirror of `crossterm::event::MouseEventKind`, reusing bevy's
/// `MouseButton` (the crossterm-compat feature maps Left/Right/Middle and
/// drops the rest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}
```

`KeyModifiers` keeps its current definition (four bools) — add
`PartialEq, Eq` to its derives.

**Feature gating:** all types above compile **unconditionally** — no
`#[cfg(feature = ...)]` on any of them, exactly like today's
`TerminalEventType`. This is load-bearing: `TerminalEvent` is the crate's
BYO injection point (§3.6 rationale), so it must exist even when both
input features are off. (`MouseButton` comes from `bevy_input`, which is
always present, so `MouseEventKind` is safe unconditionally too.) The
capture *systems* keep their existing gating layout unchanged — do not
add or move any `#[cfg]` on them; registration in `bevy_plugin.rs`
already gates per feature.

**Naming rule for `KeyCode` (important — two collision sites):**

1. *Inside `src/input/mod.rs` itself.* The module currently does
   `use bevy::prelude::*;` and uses bare `KeyCode` to mean bevy's
   physical-key enum. An item **defined** in a module always beats a
   glob import, so defining our `pub enum KeyCode` silently re-points
   every remaining bare `KeyCode` in the file at the new enum —
   `KeyCode::ControlLeft` etc. stop compiling. Fix: add
   `use bevy::input::keyboard::KeyCode as BevyKeyCode;` at the top and
   rename every surviving physical-key reference to `BevyKeyCode`. After
   the §3.8 deletions the survivors are exactly: the
   `Res<ButtonInput<BevyKeyCode>>` system params (keyboard, mouse,
   `terminal_focus_system`), the modifier reads inside `read_modifiers`
   (§3.3), `BevyKeyCode::Tab` in `terminal_focus_system`, and the
   `Unidentified(BevyKeyCode)` variant above (an alias is only a path —
   rustdoc still shows the real bevy type).
2. *Downstream, via the preludes.* `bevy::prelude::*` also exports
   `KeyCode`, and two **glob** imports of one name are ambiguous at every
   use site. Therefore `crate::prelude` re-exports `TerminalEvent`,
   `InputEvent`, `KeyEvent`, `KeyEventKind`, `MouseEvent`,
   `MouseEventKind`, `KeyModifiers` (replacing the removed
   `TerminalEventType`) but **not** `KeyCode`. Consumers write one
   explicit import — `use bevy_tui_texture::input::KeyCode;` — which
   Rust resolves in preference to any glob, cleanly shadowing bevy's.
   Document this in the prelude's comment and use the pattern in every
   migrated example. (None of the other new names exist in
   `bevy::prelude`, so they are safe to glob-export.)

### 3.2 Keyboard capture rewrite (`keyboard_input_system`)

Replace the `keyboard.get_just_pressed()` + `keycode_to_char` body:

- Add `mut key_input: MessageReader<bevy::input::keyboard::KeyboardInput>`
  as a system param. Keep `Res<ButtonInput<BevyKeyCode>>` as a param, but
  its only use becomes the single call
  `let modifiers = read_modifiers(&keyboard);` — the shared helper
  defined in §3.3-4 is the **one** modifier implementation for both
  systems; delete the inline four-line computation here.
- For each `KeyboardInput` message, emit exactly **one**
  `InputEvent::Key(KeyEvent)` (this removes the C1 double emission).
  Field facts below are verified against `bevy_input-0.19.0/src/keyboard.rs`
  (struct `KeyboardInput { key_code, logical_key, state, text, repeat,
  window }`) — no re-verification needed:
  - `kind`: `state == ButtonState::Released` → `Release`; else `Repeat`
    if `repeat` is true, else `Press`.
  - `code` from `logical_key: bevy::input::keyboard::Key`, first match
    wins:
    1. `Key::Character(s)` → `KeyCode::Char(s.chars().next())` (winit has
       already applied layout and Shift; skip the whole message if `s` is
       empty).
    2. `Key::Space` → `Char(' ')`.
    3. `Key::Tab` → `BackTab` when `modifiers.shift`, else `Tab` (see the
       accepted press/release quirk on the `BackTab` doc comment in
       §3.1 — no per-key state tracking).
    4. `Key::Enter | Backspace | Delete | Insert | Escape | ArrowLeft |
       ArrowRight | ArrowUp | ArrowDown | Home | End | PageUp | PageDown`
       → the corresponding mirror variant (`Escape` → `Esc`).
    5. The function keys are **twelve individual unit variants** on
       bevy's `Key` — range patterns are impossible; write twelve match
       arms: `Key::F1 => KeyCode::F(1),` … `Key::F12 => KeyCode::F(12),`.
       (`Key::F13`+ exist too but fall through to rule 6.)
    6. Everything else (including `Key::Dead`, modifier keys, media keys)
       → `Unidentified(key_code)` where `key_code` is the message's
       physical `key_code` field.
  - Factor the `logical_key`→`KeyCode` mapping into a pure function
    `fn keycode_from_logical(key: &Key, shift: bool, physical: BevyKeyCode) -> Option<KeyCode>`
    so it is unit-testable without an `App`. Return `None` only for the
    empty-`Character` case.
- Focus gating and `remap_to_tui` targeting: unchanged.
- **Delete** `keycode_to_char` and its tests entirely.

Note: modifier keys themselves (plain Shift press etc.) now arrive as
`Unidentified(ShiftLeft)` — same information as the old model, one arm.

### 3.3 Mouse capture changes

All three `mouse_input_system` variants (2d+3d / 2d-only / 3d-only) share
the same helpers; the changes live in the helpers plus one new param per
variant. Hit-testing, sorting, change-detection gates: untouched.

1. **Move vs Drag** — `emit_mouse_move` gains `buttons: &ButtonInput<MouseButton>`
   and `touches: &Touches` params and picks the kind via a new pure
   function:
   ```rust
   fn move_kind(left: bool, right: bool, middle: bool) -> MouseEventKind {
       if left { MouseEventKind::Drag(MouseButton::Left) }
       else if right { MouseEventKind::Drag(MouseButton::Right) }
       else if middle { MouseEventKind::Drag(MouseButton::Middle) }
       else { MouseEventKind::Moved }
   }
   ```
   where `left` is
   `buttons.pressed(MouseButton::Left) || touches.first_pressed_position().is_some()`
   — bevy 0.19's `Touches` has **no** `any_pressed()`; the "is a touch
   currently held" check is `first_pressed_position().is_some()`, the
   same call `update_cursor_position_system` already uses, so an active
   touch counts as a held left button consistent with the existing tap
   emulation. The existing `last_hovered` dedup logic is unchanged.
2. **Down/Up** — `emit_button_events` emits
   `MouseEventKind::Down(button)` / `Up(button)` instead of
   `MousePress`/`MouseRelease`. Focus handling unchanged.
3. **Scroll** — each variant gains
   `mut wheel: MessageReader<bevy::input::mouse::MouseWheel>`. Collect the
   messages into a local `Vec` at the top of the system. Two integration
   points:
   - The early-return gate must not swallow wheel input: add
     `!wheel_messages.is_empty()` to the "something happened" condition.
   - After the topmost hit is selected, for each wheel message emit one
     event at the hit cell: `y > 0.0` → `ScrollUp`, `y < 0.0` →
     `ScrollDown`; else `x > 0.0` → `ScrollRight`, `x < 0.0` →
     `ScrollLeft` (one event per `MouseWheel` message, sign only — do not
     try to accumulate deltas). Factor the sign→kind mapping into a pure
     `fn scroll_kind(x: f32, y: f32) -> Option<MouseEventKind>`.
4. **Modifiers** — define the single shared helper
   `fn read_modifiers(keyboard: &ButtonInput<BevyKeyCode>) -> KeyModifiers`
   (the four `pressed(ControlLeft) || pressed(ControlRight)`-style lines
   currently inlined in `keyboard_input_system`, moved verbatim). It is
   the **only** modifier implementation in the crate: the keyboard system
   (§3.2) and all mouse emissions call it; each mouse system variant
   gains a `Res<ButtonInput<BevyKeyCode>>` param for the purpose.

### 3.4 Resize (`window_resize_system`)

Trigger, query, and loop stay exactly as they are; only the constructed
payload changes:

```rust
InputEvent::Resize { pixels: UVec2::new(resize_event.width as u32, resize_event.height as u32) }
```

No cell counts — see the `Resize` doc comment in §3.1 for the recorded
rationale (a crossterm-style cols/rows here would report the *old* grid,
because texture terminals only resize when the caller asks).

### 3.5 Touch

No behavioral change. `update_cursor_position_system` and the tap→left-
button emulation stay exactly as they are; taps now surface as
`MouseEventKind::Down(MouseButton::Left)` / `Up(...)` and moves while
touching as `Drag(MouseButton::Left)` automatically via §3.3.

### 3.6 `HitRegions` decoupling (`src/setup.rs`)

Two changes, nothing else:

```rust
#[derive(Component, Default)]   // was: #[derive(Default)]
pub struct HitRegions { ... }

pub fn clear(&mut self) { ... } // was: fn clear
```

Rationale (record in the doc comment): `HitRegions` is pure data keyed by
`u64` — nothing about it is texture-specific. Making it spawnable as its
own component lets a display-agnostic consumer (e.g. a future
bevy_ratatui adapter, or BYO input pipelines) own a registry without a
`Tui`. The `Tui`-embedded instance and `draw_with_hits` keep working
unchanged; do not migrate them.

### 3.7 `crossterm-compat` feature

`Cargo.toml`:

```toml
[features]
crossterm-compat = ["dep:crossterm"]

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
crossterm = { version = "0.29", optional = true, default-features = false, features = ["events"] }
```

Both version and features are **verified facts** (checked against the
registry sources on 2026-07-18), not placeholders — do not re-derive
them:

- ratatui 0.30.2 routes its crossterm backend through
  `ratatui-crossterm 0.1.2`, whose default is the `crossterm_0_29`
  feature → dependency requirement `version = "0.29"`. Pinning `"0.29"`
  therefore unifies with the ratatui ecosystem's tree.
- crossterm 0.29 gates `pub mod event` behind its `"events"` feature,
  which is a *default* feature — so `default-features = false` **without**
  `features = ["events"]` would remove the very types this module
  converts to. The `events` feature pulls mio/signal-hook on unix;
  acceptable for an opt-in, native-only compat feature.

New module `src/input/crossterm_compat.rs`, gated
`#[cfg(all(feature = "crossterm-compat", not(target_arch = "wasm32")))]`,
containing exactly two public functions (both lossy, `None` =
unrepresentable — document that):

```rust
impl InputEvent {
    /// KeyCode::Unidentified, non-Left/Right/Middle mouse buttons, and
    /// Resize (pixel-based here, cell-based in crossterm — the units
    /// don't convert) return None.
    pub fn to_crossterm(&self) -> Option<crossterm::event::Event>;
    /// crossterm KeyCodes outside our mirror (media keys, keypad, ...),
    /// non-standard mouse buttons, and Resize (cell-based, see above)
    /// return None.
    pub fn from_crossterm(event: &crossterm::event::Event) -> Option<InputEvent>;
}
```

Mapping notes: `BackTab` ↔ `crossterm::event::KeyCode::BackTab`;
`KeyModifiers` bools ↔ crossterm bitflags (SHIFT/CONTROL/ALT/SUPER);
`KeyEventKind` maps 1:1 (crossterm has the same three variants);
`MouseEventKind` maps 1:1 with the button conversion. Keep the module
under 120 lines; a straightforward pair of `match` expressions.

Also add the feature to CI's `--all-features` runs (it already is, by
definition) and mention it in the README feature list.

### 3.8 Deletions

- `TerminalEventType` (the whole enum) — no deprecation shim.
- `keycode_to_char` + its 4 test functions.
- The `CharInput`-related doc comments.

## 4. Implementation plan (ordered; each step must compile and pass tests)

1. **Types + keyboard** — add §3.1 types, update the prelude exports per
   §3.1's naming rule, rewrite `keyboard_input_system` (§3.2), delete
   `keycode_to_char`. Port `window_resize_system` (§3.4)
   and the focus/`emit_focus_events` call sites (their payload variants
   changed name only). Migrate the examples' key handling (§5). Add unit
   tests for `keycode_from_logical`.
2. **Mouse** — §3.3 (Down/Up/Drag/Moved/Scroll + modifiers). Migrate the
   examples' mouse handling. Add unit tests for `move_kind` /
   `scroll_kind` / `read_modifiers`.
3. **HitRegions decoupling** — §3.6.
4. **crossterm-compat** — §3.7 + round-trip unit tests (native-only,
   `#[cfg(feature = "crossterm-compat")]`).
5. **form_demo** — create `examples/form_demo.rs` from §6, register it in
   `Cargo.toml` `[[example]]`, run it manually.
6. **Docs** — update CLAUDE.md (input section, feature list, example
   list), README event examples, and the doc comments referenced above.

## 5. Example migration map

Affected: `widget_catalog_2d`, `widget_catalog_3d`, `multiple_terminals`,
`world_terminal`, `resize`, `tui_component`, `retro_crt` (and via it
`wasm_demo`). `helloworld` and the benchmarks read no events.

Every migrated example that matches key codes needs the explicit
`use bevy_tui_texture::input::KeyCode;` import (see §3.1's prelude rule).

| Old pattern | New pattern |
|---|---|
| `event.event` | `event.input` |
| `TerminalEventType::KeyPress { key: KeyCode::KeyQ, .. }` | `InputEvent::Key(KeyEvent { code: KeyCode::Char('q'), .. })` |
| `KeyPress { key: KeyCode::ArrowUp, .. }` | `Key(KeyEvent { code: KeyCode::Up, .. })` |
| `KeyPress { key: KeyCode::Tab, .. }` | `Key(KeyEvent { code: KeyCode::Tab, .. })` (Shift+Tab is now `BackTab`) |
| `CharInput { character }` | `Key(KeyEvent { code: KeyCode::Char(c), kind: Press \| Repeat, .. })` — filter out `Release` when doing text entry |
| `MousePress { button, position: (col, row) }` | `Mouse(MouseEvent { kind: MouseEventKind::Down(button), column, row, .. })` |
| `MouseRelease { .. }` | `kind: MouseEventKind::Up(..)` |
| `MouseMove { position }` | `kind: Moved` **or** `Drag(_)` — hover code must match both (e.g. `Moved \| Drag(_)`); drag-tracking code (retro_crt's slider) can now match `Drag(MouseButton::Left)` directly and drop its own pressed-state bookkeeping |
| `Resize { new_size: (w, h) }` | `Resize { pixels }` (`pixels.x`/`pixels.y`; still the window size in logical pixels — semantics unchanged, shape only) |

Key subtlety to preserve behavior: old code matched **physical** keys
(`KeyCode::KeyQ`); new code matches **logical** characters
(`Char('q')`) — on non-QWERTY layouts the new behavior (following the
printed keycap) is the intended fix, not a regression.

## 6. `examples/form_demo.rs` (complete source)

The acceptance artifact for "forms are app code": a working form using
only `TerminalEvent` + `HitRegions`. Register in `Cargo.toml`:

```toml
[[example]]
name = "form_demo"
path = "examples/form_demo.rs"
```

```rust
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
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    if term.hit_regions().hit_at::<Widget>((m.column, m.row))
                        == Some(Widget::Plan)
                    {
                        let step = if m.kind == MouseEventKind::ScrollUp {
                            PLANS.len() - 1 // backwards, wrapping
                        } else {
                            1
                        };
                        form.plan = (form.plan + step) % PLANS.len();
                    }
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
```

## 7. Tests and acceptance criteria

Unit tests (inline `#[cfg(test)]`, matching the repo convention):

- `keycode_from_logical`: `Character("a")`→`Char('a')`;
  `Character("Ω")`→`Char('Ω')`; `Tab`+shift→`BackTab`; `F5`→`F(5)`;
  unknown named key→`Unidentified(physical)`; empty `Character`→`None`.
- `move_kind`: no buttons→`Moved`; left→`Drag(Left)`; left+right→
  `Drag(Left)` (left wins, document as arbitrary-but-stable).
- `scroll_kind`: `y>0`→`ScrollUp`, `y<0`→`ScrollDown`, `y==0,x>0`→
  `ScrollRight`, all-zero→`None`.
- `read_modifiers`: constructed `ButtonInput<BevyKeyCode>` with
  ControlLeft pressed → `ctrl` only; empty → all false.
- `crossterm-compat` (feature-gated): round-trip
  `from_crossterm(to_crossterm(e)) == Some(e)` for representable events;
  `to_crossterm` of `Unidentified(..)` key → `None`.
- Existing `pixel_to_cell` / `uv_to_cell` tests: keep unchanged.

Acceptance (all must pass):

```bash
cargo test --all-features
cargo clippy --all-features --all-targets -- -D warnings
cargo doc --no-deps --all-features
cargo check --lib --no-default-features
cargo check --lib --no-default-features --features 2d
cargo check --lib --no-default-features --features 3d
cargo check --target wasm32-unknown-unknown --example wasm_demo
cargo build --examples   # every migrated example + form_demo compiles
```

Manual smoke test: `cargo run --example form_demo` — click focuses,
typing edits Name, wheel cycles Plan, Tab/Shift+Tab cycle, Submit prints
to the status line. `cargo run --example retro_crt` — slider drag works
via `Drag(Left)`.

LOC guardrail: `git diff --stat` for `src/` should be near net-zero
(additions offset by the deleted key table); if `src/input/mod.rs` grows
by more than ~150 lines, simplify before proceeding.

## 8. Non-goals and future work (recorded decisions)

- **No form/validation framework in this crate.** Decided 2026-07: the
  crate's identity is transport + rendering, mirroring bevy_ratatui's
  mental model. Survey findings that support building forms *outside*
  later: [schemaui](https://github.com/YuniqueUnic/schemaui) (JSON Schema
  → ratatui form tree with validation; accepts TOML/YAML/JSON; active,
  v0.7.5) covers the schema/validation half;
  [rat-widget](https://github.com/thscharler/rat-salsa) covers input
  widgets; a future `bevy_tui_forms` crate could combine them on top of
  `TerminalEvent` + `HitRegions` without any core changes — `hit_at` is
  already generic over `TryFrom<u64>`, and §3.6 makes `HitRegions`
  spawnable standalone.
- **No bevy_ratatui adapter in-tree.** Feasible later as a ~100-line
  external crate: bevy_ratatui's `KeyMessage` etc. → `from_crossterm` →
  `TerminalEvent { target: marker_entity, .. }`. The crossterm-shaped
  payload (§3.1) and `HitRegions` decoupling (§3.6) are the only
  prerequisites, and both ship in 0.4.
- **No IME composition events.** `Key::Character` covers committed text
  reachable through winit's logical keys. A future `InputEvent::Ime(..)`
  variant remains possible: the enum is deliberately NOT
  `#[non_exhaustive]` (exhaustive matching is a feature for consumers),
  so adding a variant is a breaking change shipped in a 0.x version bump
  like any other — document this trade-off in the enum's doc comment.
- **No unconditional crossterm dependency** (wasm), and **no deprecation
  shim** for `TerminalEventType` (clean break, migration table in §5).

## Appendix: pre-implementation review — findings and resolutions

A spec review (2026-07-18) raised nine findings; each is resolved
**in the sections above** (this table is a decision log, not extra
instructions — if it ever disagrees with a section, the section wins).

| # | Finding | Resolution (where) |
|---|---------|--------------------|
| 1 | New `KeyCode` collides with bare bevy `KeyCode` uses inside `src/input/mod.rs` itself (module items beat glob imports) | `use bevy::input::keyboard::KeyCode as BevyKeyCode;` + rename the enumerated surviving sites (§3.1 naming rule, site list included) |
| 2 | §3.2 said "keep inline modifiers", §3.3 said "share `read_modifiers`" | Shared `read_modifiers` is the single implementation; keyboard system calls it too (§3.2, §3.3-4) |
| 3 | `Resize { cols, rows }` would report the *old* grid, contradicting crossterm's "new size" semantics | Fields dropped: `Resize { pixels: UVec2 }` only; rationale recorded on the variant doc; crossterm-compat maps Resize → `None` both ways (§3.1, §3.4, §3.7, C4) |
| 4 | BackTab press/release mismatch when Shift is released first | Accepted and documented on the `BackTab` doc comment; explicitly no per-key state tracking (§3.1, §3.2 rule 3) |
| 5 | `Key::F1..F12` cannot be a Rust range pattern | Spelled out: twelve individual match arms; `F13`+ falls through to `Unidentified` (§3.2 rule 5) |
| 6 | `Touches::any_pressed()` does not exist in bevy 0.19 | Exact expression specified: `first_pressed_position().is_some()` (§3.3-1) |
| 7 | Feature-gating of the new types unspecified | All §3.1 types compile unconditionally (BYO injection point); capture-system gating layout untouched (§3.1) |
| 8 | crossterm `"0.29"` was an unverified placeholder; `default-features = false` alone drops the `event` module | Verified against registry sources: ratatui-crossterm 0.1.2 defaults to crossterm 0.29, and `features = ["events"]` is mandatory (§3.7) |
| 9 | `InputEvent` lacked `Eq` while its members had it | `Eq` added to `TerminalEvent` and `InputEvent` (`UVec2` derives `Eq`/`Hash` — verified in glam 0.32.1) (§3.1) |
