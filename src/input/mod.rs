//! Event-driven terminal input via Bevy's ECS.
//!
//! Bevy input systems → `TerminalEvent` messages → User systems → Terminal updates
//!
//! The envelope (`TerminalEvent`, entity-targeted) is Bevy-native; the
//! payload (`InputEvent`) mirrors `crossterm::event::Event` so the wider
//! ratatui ecosystem's input vocabulary (tui-textarea, bevy_ratatui, etc.)
//! maps onto it directly. `InputEvent`/`KeyEvent`/`KeyCode`/`MouseEvent`
//! are self-defined mirrors, not a crossterm dependency, so wasm builds
//! stay intact; see the `crossterm-compat` feature (native-only) for real
//! conversions to and from `crossterm::event::Event`.
//!
//! This module is deliberately just transport: no form/widget framework
//! lives here, or ever will. `crate::setup::HitRegions` (a `u64`-keyed
//! click-region registry, generic over any `TryFrom<u64>` id type) is as
//! far as it goes - see `examples/form_demo.rs` for a complete interactive
//! form built on nothing but that plus this module's `TerminalEvent`.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
// Bevy's own `KeyCode` (physical key location) is aliased rather than used
// bare: this module also defines its own `pub enum KeyCode` (the
// crossterm-shaped logical/character mirror below), and an item defined in
// a module always wins name resolution over a glob-imported one - so every
// use of bevy's physical KeyCode in this file must go through this alias,
// never the bare name.
use bevy::input::keyboard::KeyCode as BevyKeyCode;
#[cfg(feature = "mouse_input")]
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
#[cfg(feature = "mouse_input")]
use tracing::debug;
//use bevy::log::debug;
//use log::debug;

// Ray casting for 3D mouse input
#[cfg(all(feature = "mouse_input", feature = "3d"))]
pub mod ray;

// Lossy conversions to/from crossterm::event::Event (native-only).
#[cfg(all(feature = "crossterm-compat", not(target_arch = "wasm32")))]
pub mod crossterm_compat;

// ============================================================================
// Events
// ============================================================================

/// Message emitted for terminal input.
///
/// These messages are emitted by input capture systems and read by user systems
/// to handle terminal input. Messages are entity-targeted, enabling selective
/// routing to specific terminal instances.
///
/// Compiles unconditionally (no feature gate): this is the crate's
/// bring-your-own-input injection point, so it must exist even when
/// `keyboard_input`/`mouse_input` are both disabled - a caller can write
/// `TerminalEvent`s from any source (an adapter for another input
/// backend, a test harness, a network channel) via `MessageWriter`.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct TerminalEvent {
    /// The Tui entity that should receive this message.
    pub target: Entity,
    pub input: InputEvent,
}

/// Mirror of `crossterm::event::Event`. Self-defined because crossterm does
/// not build on wasm32-unknown-unknown; see the `crossterm-compat` feature
/// for lossy conversions to/from the real crossterm type on native.
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
    /// actually need (see `examples/resize.rs`); grid changes are always
    /// caller-initiated, so the caller already knows them.
    Resize { pixels: UVec2 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

/// Mirror of `crossterm::event::KeyCode` (the subset a GPU-windowed app can
/// produce), plus an escape hatch for everything else. Not `KeyCode` from
/// `bevy::prelude` - see the module-level import comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    /// Shift+Tab, following crossterm's convention (`modifiers.shift`
    /// stays true as well). Known, accepted quirk: derived from the
    /// *live* shift state at each key transition, so releasing Shift
    /// before releasing Tab yields a `Press` of `BackTab` but a
    /// `Release` of `Tab`. Do not try to fix this with per-key state
    /// tracking - crossterm has the same ambiguity, and the standard
    /// consumer pattern (ignore `Release` events) never observes it.
    BackTab,
    Backspace,
    Delete,
    Insert,
    Esc,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    /// Anything not representable above - carries the physical key so no
    /// information is lost. NOT part of the crossterm mirror; converts to
    /// `None` in `crossterm-compat`.
    Unidentified(BevyKeyCode),
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
/// `MouseButton` directly (the `crossterm-compat` feature maps
/// Left/Right/Middle and drops the rest).
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

/// Modifier keys state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    /// Control key pressed
    pub ctrl: bool,
    /// Alt/Option key pressed
    pub alt: bool,
    /// Shift key pressed
    pub shift: bool,
    /// Meta key pressed (Command on Mac, Windows key on Windows/Linux)
    pub meta: bool,
}

// ============================================================================
// Resources
// ============================================================================

/// Tracks which terminal currently has input focus.
///
/// Only one terminal can have keyboard focus at a time.
/// Keyboard events are routed only to the focused terminal.
///
/// Focus can be changed by:
/// - Clicking on a terminal (automatic)
/// - Pressing Tab key (cycles through terminals with `TerminalInput`)
/// - Manually setting `focus.focused = Some(entity)`
#[derive(Resource, Default, Debug)]
pub struct TerminalFocus {
    /// Entity of the currently focused terminal, or None if no terminal has focus
    pub focused: Option<Entity>,
}

/// Global config for terminal input. Inserted by `TerminalPlugin`.
#[derive(Resource, Clone, Debug)]
pub struct TerminalInputConfig {
    /// Enable keyboard input capture
    pub keyboard_enabled: bool,
    /// Enable mouse input capture
    pub mouse_enabled: bool,
    /// Enable automatic focus management (Tab key cycling)
    pub auto_focus: bool,
    /// Mouse button used for focus/selection
    pub focus_button: MouseButton,
}

impl Default for TerminalInputConfig {
    fn default() -> Self {
        Self {
            keyboard_enabled: true,
            mouse_enabled: true,
            auto_focus: true,
            focus_button: MouseButton::Left,
        }
    }
}

/// Cached cursor position in window coordinates.
///
/// Updated by `update_cursor_position_system` and used by `mouse_input_system`
/// for hit-testing. On touch devices (where no mouse cursor exists) this
/// holds the first active touch's position instead - see
/// [`update_cursor_position_system`].
#[derive(Resource, Default, Debug)]
pub struct CursorPosition {
    /// Current cursor position, or None if cursor is outside window
    pub position: Option<Vec2>,
}

// ============================================================================
// Components
// ============================================================================

/// Enable input routing for a terminal. Without this component, terminals are display-only.
#[derive(Component, Debug, Clone)]
pub struct TerminalInput {
    /// Whether this terminal can receive keyboard input
    pub keyboard: bool,
    /// Whether this terminal can receive mouse input
    pub mouse: bool,
}

impl Default for TerminalInput {
    fn default() -> Self {
        Self {
            keyboard: true,
            mouse: true,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Reads the four modifier keys from bevy's `ButtonInput`. The single
/// modifier implementation in the crate - both `keyboard_input_system` and
/// every `mouse_input_system` variant call this rather than each computing
/// their own.
fn read_modifiers(keyboard: &ButtonInput<BevyKeyCode>) -> KeyModifiers {
    KeyModifiers {
        ctrl: keyboard.pressed(BevyKeyCode::ControlLeft) || keyboard.pressed(BevyKeyCode::ControlRight),
        alt: keyboard.pressed(BevyKeyCode::AltLeft) || keyboard.pressed(BevyKeyCode::AltRight),
        shift: keyboard.pressed(BevyKeyCode::ShiftLeft) || keyboard.pressed(BevyKeyCode::ShiftRight),
        meta: keyboard.pressed(BevyKeyCode::SuperLeft) || keyboard.pressed(BevyKeyCode::SuperRight),
    }
}

/// Maps bevy's logical key (already layout/Shift-resolved by winit) to the
/// crossterm-shaped `KeyCode`. Pure and unit-testable without an `App`.
/// `shift` decides `Tab` vs `BackTab` (see `KeyCode::BackTab`'s doc comment
/// for the accepted press/release quirk this implies). Returns `None` only
/// when `Key::Character` carries no characters - a winit edge case, not
/// expected in practice - in which case the caller should skip the whole
/// `KeyboardInput` message.
fn keycode_from_logical(key: &Key, shift: bool, physical: BevyKeyCode) -> Option<KeyCode> {
    Some(match key {
        Key::Character(s) => KeyCode::Char(s.chars().next()?),
        Key::Space => KeyCode::Char(' '),
        Key::Tab if shift => KeyCode::BackTab,
        Key::Tab => KeyCode::Tab,
        Key::Enter => KeyCode::Enter,
        Key::Backspace => KeyCode::Backspace,
        Key::Delete => KeyCode::Delete,
        Key::Insert => KeyCode::Insert,
        Key::Escape => KeyCode::Esc,
        Key::ArrowLeft => KeyCode::Left,
        Key::ArrowRight => KeyCode::Right,
        Key::ArrowUp => KeyCode::Up,
        Key::ArrowDown => KeyCode::Down,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::F1 => KeyCode::F(1),
        Key::F2 => KeyCode::F(2),
        Key::F3 => KeyCode::F(3),
        Key::F4 => KeyCode::F(4),
        Key::F5 => KeyCode::F(5),
        Key::F6 => KeyCode::F(6),
        Key::F7 => KeyCode::F(7),
        Key::F8 => KeyCode::F(8),
        Key::F9 => KeyCode::F(9),
        Key::F10 => KeyCode::F(10),
        Key::F11 => KeyCode::F(11),
        Key::F12 => KeyCode::F(12),
        _ => KeyCode::Unidentified(physical),
    })
}

// ============================================================================
// Event-target remapping
// ============================================================================

/// Remap a hit-tested / focused entity to the [`Tui`](crate::setup::Tui)
/// entity it displays, via [`TuiSurface`](crate::setup::TuiSurface).
///
/// For library-spawned terminals (`tui == surface`) and for any entity
/// without a `TuiSurface` component at all, this is the identity -
/// `TerminalEvent::target` is unchanged. Only attached terminals (see
/// `AttachTerminal`), where the surface entity (carrying
/// `TerminalInput`/`TerminalDimensions`, where hit-testing happens) differs
/// from the `Tui` entity, actually remap.
fn remap_to_tui(entity: Entity, surfaces: &Query<&crate::setup::TuiSurface>) -> Entity {
    surfaces.get(entity).map(|s| s.tui).unwrap_or(entity)
}

// ============================================================================
// Input Systems
// ============================================================================

/// Update cursor position from window.
///
/// This system reads the cursor position from the primary window and updates
/// the `CursorPosition` resource for use by other input systems.
///
/// Falls back to the first active touch's position when no mouse cursor is
/// available: on touch devices (mobile browsers in particular) winit reports
/// taps as touch events only and never moves a mouse cursor, so without
/// this fallback a tap would never hit-test at all. Touch positions arrive
/// in the same logical-pixel, top-left-origin window space as
/// `Window::cursor_position()` (bevy_winit converts with `to_logical`).
/// The frame a touch ends, `first_pressed_position` is already `None`, but
/// `mouse_input_system` still needs a position to emit `MouseEventKind::Up`
/// at - hence the second fallback to the just-released touch's last
/// position.
pub fn update_cursor_position_system(
    mut cursor_pos: ResMut<CursorPosition>,
    windows: Query<&Window>,
    touches: Res<Touches>,
) {
    // Get primary window
    if let Ok(window) = windows.single() {
        cursor_pos.position = window
            .cursor_position()
            .or_else(|| touches.first_pressed_position())
            .or_else(|| touches.iter_just_released().next().map(|t| t.position()));
    }
}

/// Keyboard input capture system.
///
/// Captures keyboard input and emits `TerminalEvent`s for the focused terminal.
/// Only processes input if a terminal has focus and has keyboard input enabled.
pub fn keyboard_input_system(
    mut key_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    focus: Res<TerminalFocus>,
    terminals: Query<&TerminalInput>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
) {
    // Check if any terminal has focus
    let Some(focused_entity) = focus.focused else {
        return;
    };

    // Check if focused terminal accepts keyboard input
    let Ok(input) = terminals.get(focused_entity) else {
        return;
    };

    if !input.keyboard {
        return;
    }

    // `focus.focused` stores the surface entity (where TerminalInput lives);
    // remap to the Tui entity only for the emitted event's target.
    let target = remap_to_tui(focused_entity, &surfaces);
    let modifiers = read_modifiers(&keyboard);

    for key_event in key_events.read() {
        let kind = match key_event.state {
            ButtonState::Released => KeyEventKind::Release,
            ButtonState::Pressed if key_event.repeat => KeyEventKind::Repeat,
            ButtonState::Pressed => KeyEventKind::Press,
        };
        let Some(code) = keycode_from_logical(&key_event.logical_key, modifiers.shift, key_event.key_code)
        else {
            continue;
        };

        events.write(TerminalEvent {
            target,
            input: InputEvent::Key(KeyEvent { code, modifiers, kind }),
        });
    }
}

// ============================================================================
// Mouse Input - Unified System Helpers
// ============================================================================

/// Terminal type detected from components. Only meaningful when both mesh
/// (3d) and UI (2d) terminals can coexist - the single-feature builds below
/// know their terminal kind up front and skip detection entirely.
#[cfg(all(feature = "mouse_input", feature = "2d", feature = "3d"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalType {
    /// 3D mesh terminal (has Mesh2d or Mesh3d component)
    Mesh3D,
    /// 2D UI terminal (has Node component)
    UI2D,
    /// Unknown terminal type (has neither mesh nor node)
    Unknown,
}

/// Result of a successful hit test.
#[cfg(feature = "mouse_input")]
#[derive(Debug, Clone, Copy)]
struct HitTestResult {
    /// Terminal grid column (0-based)
    col: u16,
    /// Terminal grid row (0-based)
    row: u16,
}

/// Sort key for selecting topmost terminal from multiple hits.
#[cfg(feature = "mouse_input")]
#[derive(Debug, Clone, Copy, PartialEq)]
enum SortKey {
    /// Z-index for 2D UI terminals (higher = on top)
    ZIndex(i32),
    /// 3D mesh terminal hit: camera priority (0 = topmost-rendered camera,
    /// i.e. highest `Camera::order`), then ray distance (lower = closer).
    Distance {
        camera_priority: usize,
        distance: f32,
    },
}

#[cfg(feature = "mouse_input")]
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (SortKey::ZIndex(a), SortKey::ZIndex(b)) => b.partial_cmp(a), // Higher Z on top
            (
                SortKey::Distance {
                    camera_priority: ca,
                    distance: da,
                },
                SortKey::Distance {
                    camera_priority: cb,
                    distance: db,
                },
            ) => {
                // Terminals seen by a higher-order (overlay) camera win;
                // within the same camera, closer hits win.
                match ca.cmp(cb) {
                    std::cmp::Ordering::Equal => da.partial_cmp(db),
                    ord => Some(ord),
                }
            }
            // A 2D UI terminal and a 3D mesh terminal can both register a
            // hit for the same cursor position: the mesh ray-cast tests
            // world-space geometry regardless of what bevy_ui has drawn on
            // top of it, so a UI panel overlapping a 3D terminal in screen
            // space (e.g. a HUD panel sitting above where a world model
            // happens to project to) produces hit_candidates for both.
            // Previously this returned `None`, and the caller's
            // `sort_by(|a, b| ... .unwrap_or(Equal))` treated that as
            // "equal" - `Vec::sort_by` is stable, so the winner silently
            // fell back to whichever entity the `terminals` query happened
            // to iterate first (archetype/spawn-order dependent, NOT
            // screen-depth dependent). That made 2D UI clicks
            // unpredictably "shadowed" by an unrelated 3D hit - observed as
            // "the 3D model's screen responds to clicks but the 2D overlay
            // panel doesn't" on some platforms/window sizes but not others,
            // purely from incidental query ordering. bevy_ui is rendered as
            // a screen-space overlay on top of every 3D camera regardless
            // of `Camera::order`, so a UI hit always wins when both fire.
            (SortKey::ZIndex(_), SortKey::Distance { .. }) => Some(std::cmp::Ordering::Less),
            (SortKey::Distance { .. }, SortKey::ZIndex(_)) => Some(std::cmp::Ordering::Greater),
        }
    }
}

/// Bundles the unified system's three change-detection probe queries
/// (IMPROVEMENT.md D1) into a single `SystemParam`. Bevy's generated
/// `SystemParam` tuple impl tops out at 16 elements; the unified variant's
/// own parameter list already sits at that ceiling once `keyboard` and
/// `wheel` are added for scroll/modifier support, so these three
/// low-frequency queries are folded into one slot instead of three.
///
/// `pub`, not `pub(crate)`: it appears in `mouse_input_system`'s (public)
/// parameter list, so it must be at least as visible as that function -
/// callers never construct it themselves (bevy injects it), so this is
/// purely to satisfy the visibility checker.
#[cfg(all(feature = "mouse_input", feature = "2d", feature = "3d"))]
#[derive(bevy::ecs::system::SystemParam)]
pub struct MouseChangeProbes<'w, 's> {
    camera: Query<
        'w,
        's,
        (),
        (
            With<Camera>,
            Or<(Changed<GlobalTransform>, Changed<Projection>, Changed<Camera>)>,
        ),
    >,
    terminal_3d: Query<'w, 's, (), (With<TerminalInput>, Changed<GlobalTransform>)>,
    terminal_ui: Query<
        'w,
        's,
        (),
        (
            With<TerminalInput>,
            Or<(Changed<bevy::ui::ComputedNode>, Changed<bevy::ui::UiGlobalTransform>)>,
        ),
    >,
}

#[cfg(all(feature = "mouse_input", feature = "2d", feature = "3d"))]
impl MouseChangeProbes<'_, '_> {
    fn any_changed(&self) -> bool {
        !self.camera.is_empty() || !self.terminal_3d.is_empty() || !self.terminal_ui.is_empty()
    }
}

/// Detect terminal type from components.
///
/// Priority: 3D mesh > 2D UI (for hybrid entities)
#[cfg(all(feature = "mouse_input", feature = "2d", feature = "3d"))]
fn detect_terminal_type(
    mesh2d: Option<&Mesh2d>,
    mesh3d: Option<&Mesh3d>,
    node: Option<&bevy::ui::Node>,
) -> TerminalType {
    if mesh3d.is_some() || mesh2d.is_some() {
        return TerminalType::Mesh3D;
    }

    if node.is_some() {
        TerminalType::UI2D
    } else {
        TerminalType::Unknown
    }
}

/// Convert a pixel position local to a terminal's rendered area (origin at
/// the terminal's top-left, `+Y` down) to a clamped grid cell. Pure
/// function, no bevy types - used by 2D UI hit-testing
/// (`bounding_box_hit_test`).
///
/// Clamps in both directions: negative input (shouldn't happen at the
/// current call site, which already bounds-checks before calling this, but
/// this function makes no such assumption) clamps to column/row 0; input at
/// or beyond the terminal's pixel size clamps to the last column/row
/// (`cols - 1`/`rows - 1`), never `cols`/`rows` themselves - those would be
/// one past the last valid grid index.
#[cfg(all(feature = "mouse_input", feature = "2d"))]
fn pixel_to_cell(local_x: f32, local_y: f32, char_width: f32, char_height: f32, cols: u16, rows: u16) -> (u16, u16) {
    let max_col = cols.saturating_sub(1) as f32;
    let max_row = rows.saturating_sub(1) as f32;
    let col = (local_x.max(0.0) / char_width).min(max_col) as u16;
    let row = (local_y.max(0.0) / char_height).min(max_row) as u16;
    (col, row)
}

/// Convert a mesh UV coordinate to a clamped grid cell. Pure function, no
/// bevy types - used by 3D ray-mesh hit-testing (`ray_cast_hit_test_inner`).
///
/// UV is nominally `0.0..=1.0`, but ray-mesh intersection can return values
/// fractionally outside that range at triangle edges due to floating-point
/// error - clamped the same way as [`pixel_to_cell`], including the "never
/// return `cols`/`rows` themselves" rule (a naive `(uv * cols).clamp(0.0,
/// cols)` allows exactly that at `uv == 1.0`, one past the last valid grid
/// index).
#[cfg(all(feature = "mouse_input", feature = "3d"))]
fn uv_to_cell(uv_x: f32, uv_y: f32, cols: u16, rows: u16) -> (u16, u16) {
    let max_col = cols.saturating_sub(1) as f32;
    let max_row = rows.saturating_sub(1) as f32;
    let col = (uv_x.max(0.0) * cols as f32).min(max_col) as u16;
    let row = (uv_y.max(0.0) * rows as f32).min(max_row) as u16;
    (col, row)
}

/// Perform 2D UI bounding box hit test.
///
/// Converts cursor position to terminal grid coordinates using UI layout bounds.
#[cfg(all(feature = "mouse_input", feature = "2d"))]
fn bounding_box_hit_test(
    cursor_pos: bevy::math::Vec2,
    ui_transform: Option<&bevy::ui::UiGlobalTransform>,
    node: Option<&bevy::ui::Node>,
    computed: Option<&bevy::ui::ComputedNode>,
    dimensions: Option<&crate::bevy_plugin::TerminalDimensions>,
) -> Option<HitTestResult> {
    // Get terminal size and scale factor
    // ComputedNode.size is in physical pixels, we need to convert to logical
    let (width_px, height_px, inverse_scale) = if let Some(comp) = computed {
        let node_size = comp.unrounded_size();
        // Convert from physical to logical pixels using inverse_scale_factor
        (
            node_size.x * comp.inverse_scale_factor,
            node_size.y * comp.inverse_scale_factor,
            comp.inverse_scale_factor,
        )
    } else {
        let n = node?;
        match (n.width, n.height) {
            (bevy::ui::Val::Px(w), bevy::ui::Val::Px(h)) => (w, h, 1.0),
            _ => return None, // Not in pixels
        }
    };

    // Coordinate system handling for Bevy UI:
    // - Cursor (input) is in UI coordinates: top-left origin, +Y down, pixels.
    // - `UiGlobalTransform` (bevy 0.19's UI-specific transform - plain
    //   `GlobalTransform` is NOT among `Node`'s required components and is
    //   simply absent on pure-UI entities) resolves to the same top-left
    //   origin, +Y down, pixel space as the cursor, but gives the node's
    //   CENTER - so it works for ANY anchor mode (left/top, right/bottom,
    //   percent, ...), unlike reading `Node.left`/`Node.top` directly (which
    //   silently reads 0 for any axis anchored from the opposite edge).
    let (node_min_x, node_min_y) = if let Some(t) = ui_transform {
        // `UiGlobalTransform::translation` is in the same physical-pixel
        // space as `ComputedNode.size` (bevy 0.19's taffy layout runs
        // entirely in physical pixels) - must be scaled to logical pixels
        // by the same `inverse_scale_factor` as `width_px`/`height_px`
        // above, or the computed bounds are wrong by exactly the window's
        // scale factor. Harmless at scale_factor 1.0 (common on non-Retina
        // Windows/Linux setups), but on macOS's default Retina
        // scale_factor of 2.0 this put the node's bounding box at 2x its
        // real logical position, so clicks always missed - reproduced both
        // natively and in wasm (wasm on a Retina Mac also reports
        // `devicePixelRatio` 2).
        let center = t.translation * inverse_scale;
        debug!(
            "UiGlobalTransform center=({:.1}, {:.1}), size=({:.1}x{:.1})",
            center.x, center.y, width_px, height_px
        );
        (center.x - width_px / 2.0, center.y - height_px / 2.0)
    } else if let Some(n) = node {
        // Fallback only correct for explicitly left/top-anchored nodes.
        let left_px = match n.left {
            bevy::ui::Val::Px(v) => v,
            _ => 0.0,
        };
        let top_px = match n.top {
            bevy::ui::Val::Px(v) => v,
            _ => 0.0,
        };
        debug!(
            "Hit test using Node.left/top fallback: pos=({:.1}, {:.1}), size=({:.1}x{:.1})",
            left_px, top_px, width_px, height_px
        );
        (left_px, top_px)
    } else {
        debug!(
            "Hit test using origin fallback: size=({:.1}x{:.1})",
            width_px, height_px
        );
        (0.0, 0.0)
    };

    let node_max_x = node_min_x + width_px;
    let node_max_y = node_min_y + height_px;

    // Check if cursor is within terminal bounds
    let x_in = cursor_pos.x >= node_min_x && cursor_pos.x <= node_max_x;
    let y_in = cursor_pos.y >= node_min_y && cursor_pos.y <= node_max_y;

    if !x_in || !y_in {
        return None; // Miss
    }

    // Get terminal dimensions and font metrics
    let (cols, rows, char_width, char_height) = if let Some(dims) = dimensions {
        (
            dims.cols as f32,
            dims.rows as f32,
            dims.char_width_px as f32,
            dims.char_height_px as f32,
        )
    } else {
        // Fallback to defaults
        let cols = 80.0;
        let rows = 24.0;
        (cols, rows, width_px / cols, height_px / rows)
    };

    // Convert screen coordinates to terminal-local coordinates
    let local_x = cursor_pos.x - node_min_x;
    let local_y = cursor_pos.y - node_min_y;

    debug!(
        "Hit test conversion: cursor=({:.1}, {:.1}), bounds=({:.1},{:.1})-({:.1},{:.1}), local=({:.1}, {:.1}), char_size=({:.1}x{:.1})",
        cursor_pos.x,
        cursor_pos.y,
        node_min_x,
        node_min_y,
        node_max_x,
        node_max_y,
        local_x,
        local_y,
        char_width,
        char_height
    );

    // Convert to terminal grid coordinates
    let (col, row) = pixel_to_cell(local_x, local_y, char_width, char_height, cols as u16, rows as u16);

    debug!("Hit test result: col={}, row={}", col, row);

    Some(HitTestResult { col, row })
}

/// Coarse pre-check (IMPROVEMENT.md D1): does `world_ray` even reach the
/// mesh entity's bounding box? Rejects most terminals with a few float ops
/// before any mesh vertex data (positions/indices/UVs) is touched by the
/// precise test in `ray_cast_hit_test_inner`.
///
/// Fails OPEN (`true`, i.e. "might hit, don't prune") whenever it lacks the
/// information to answer confidently - no `Aabb` component, or a
/// degenerate (e.g. zero-scale) transform - since pruning must never
/// introduce a false negative that silently drops a real hit.
///
/// `Aabb` is stored in the mesh's own LOCAL/model space (see its doc
/// comment in `bevy_camera::primitives`), so this transforms the
/// WORLD-space ray into that same local space via the inverse of
/// `mesh_transform`'s affine matrix - mirroring exactly how
/// `ray_mesh_intersection` itself handles `mesh_transform` (it inverts the
/// same affine and transforms the ray, not the mesh vertices), so scale,
/// rotation, and translation are all accounted for consistently between
/// the coarse and precise stages.
#[cfg(all(feature = "mouse_input", feature = "3d"))]
fn ray_intersects_aabb(
    world_ray: &crate::input::ray::Ray,
    mesh_transform: &bevy::transform::components::GlobalTransform,
    aabb: Option<&bevy::camera::primitives::Aabb>,
) -> bool {
    use bevy::math::bounding::{Aabb3d, RayCast3d};
    use bevy::math::{Dir3A, Vec3A};

    let Some(aabb) = aabb else {
        return true;
    };

    let world_to_local = mesh_transform.affine().inverse();
    let local_origin = world_to_local.transform_point3(world_ray.origin);
    let local_direction = world_to_local.transform_vector3(world_ray.direction);
    let Ok(local_direction) = Dir3A::try_from(Vec3A::from(local_direction)) else {
        return true;
    };

    let ray_cast = RayCast3d::new(local_origin, local_direction, f32::MAX);
    let aabb3d = Aabb3d::new(aabb.center, aabb.half_extents);
    ray_cast.aabb_intersection_at(&aabb3d).is_some()
}

/// Perform 3D ray-mesh hit test.
///
/// Converts cursor position to terminal grid coordinates using ray casting and UV mapping.
/// Works with both Mesh2d and Mesh3d by accepting the inner Handle<Mesh>.
#[cfg(all(feature = "mouse_input", feature = "3d"))]
fn ray_cast_hit_test_inner(
    world_ray: &crate::input::ray::Ray,
    mesh_transform: &bevy::transform::components::GlobalTransform,
    mesh_handle: &bevy::asset::Handle<bevy::mesh::Mesh>,
    meshes: &bevy::asset::Assets<bevy::mesh::Mesh>,
    dimensions: Option<&crate::bevy_plugin::TerminalDimensions>,
) -> Option<(HitTestResult, f32)> {
    use bevy::math::Ray3d;
    use bevy::mesh::VertexAttributeValues;
    use bevy::picking::mesh_picking::ray_cast::{Backfaces, ray_mesh_intersection};
    use bevy::prelude::*;

    let mesh = meshes.get(mesh_handle)?;

    let ray3d = Ray3d::new(
        world_ray.origin,
        bevy::math::Dir3::new_unchecked(world_ray.direction),
    );
    let mesh_transform_affine = mesh_transform.affine();

    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };

    let vertex_normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(normals)) => Some(normals.as_slice()),
        _ => None,
    };

    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uv_data)) => Some(uv_data.as_slice()),
        _ => None,
    };

    let indices_vec: Option<Vec<usize>> = mesh.indices().map(|idx| idx.iter().collect());
    let hit = if let Some(ref indices) = indices_vec {
        ray_mesh_intersection(
            ray3d,
            &mesh_transform_affine,
            positions.as_slice(),
            vertex_normals,
            Some(indices.as_slice()),
            uvs,
            Backfaces::Cull,
        )
    } else {
        ray_mesh_intersection(
            ray3d,
            &mesh_transform_affine,
            positions.as_slice(),
            vertex_normals,
            None::<&[u32]>,
            uvs,
            Backfaces::Cull,
        )
    }?;

    let uv = hit.uv?;

    let (cols, rows) = if let Some(dims) = dimensions {
        (dims.cols, dims.rows)
    } else {
        (80, 24)
    };

    // UV to terminal grid mapping (90° CCW rotated mesh)
    let (col, row) = uv_to_cell(uv.x, uv.y, cols, rows);

    debug!(
        "3D Hit Test: uv=({:.3},{:.3}) distance={:.1} cols={} rows={} -> grid=({},{})",
        uv.x, uv.y, hit.distance, cols, rows, col, row
    );

    Some((HitTestResult { col, row }, hit.distance))
}

/// Selects `Moved` vs `Drag(button)` from which mouse buttons (and touches,
/// pre-folded into `left` by the caller) are currently held. Left wins if
/// multiple buttons are held at once - arbitrary but stable.
#[cfg(feature = "mouse_input")]
fn move_kind(left: bool, right: bool, middle: bool) -> MouseEventKind {
    if left {
        MouseEventKind::Drag(MouseButton::Left)
    } else if right {
        MouseEventKind::Drag(MouseButton::Right)
    } else if middle {
        MouseEventKind::Drag(MouseButton::Middle)
    } else {
        MouseEventKind::Moved
    }
}

/// Selects a `MouseEventKind::Scroll*` from a `MouseWheel` message's raw
/// `(x, y)` delta - sign only, no accumulation. `y` takes priority over
/// `x` (matches how a plain vertical-wheel mouse reports). `None` when
/// both deltas are exactly zero (shouldn't normally happen for a real
/// `MouseWheel` message, but callers must not assume every message
/// produces an event).
#[cfg(feature = "mouse_input")]
fn scroll_kind(x: f32, y: f32) -> Option<MouseEventKind> {
    if y > 0.0 {
        Some(MouseEventKind::ScrollUp)
    } else if y < 0.0 {
        Some(MouseEventKind::ScrollDown)
    } else if x > 0.0 {
        Some(MouseEventKind::ScrollRight)
    } else if x < 0.0 {
        Some(MouseEventKind::ScrollLeft)
    } else {
        None
    }
}

#[cfg(feature = "mouse_input")]
#[allow(clippy::too_many_arguments)]
fn emit_mouse_move(
    surface_entity: Entity,
    col: u16,
    row: u16,
    buttons: &ButtonInput<MouseButton>,
    touches: &Touches,
    modifiers: KeyModifiers,
    surfaces: &Query<&crate::setup::TuiSurface>,
    events: &mut MessageWriter<TerminalEvent>,
) {
    // An active touch counts as a held left button - consistent with the
    // tap-emulates-left-click convention in `emit_button_events` below, and
    // reuses the same "is a touch currently held" check
    // `update_cursor_position_system` already relies on (bevy's `Touches`
    // has no `any_pressed()`).
    let left = buttons.pressed(MouseButton::Left) || touches.first_pressed_position().is_some();
    let kind = move_kind(
        left,
        buttons.pressed(MouseButton::Right),
        buttons.pressed(MouseButton::Middle),
    );
    events.write(TerminalEvent {
        target: remap_to_tui(surface_entity, surfaces),
        input: InputEvent::Mouse(MouseEvent {
            kind,
            column: col,
            row,
            modifiers,
        }),
    });
}

/// `old_focus`/`new_focus` are surface entities (what `TerminalFocus` stores,
/// matching keyboard_input_system's `TerminalInput` lookup); the emitted
/// `TerminalEvent::target` is remapped to each side's Tui entity.
#[cfg(feature = "mouse_input")]
fn emit_focus_events(
    new_focus: Entity,
    old_focus: &mut Option<Entity>,
    focus_button: MouseButton,
    button: MouseButton,
    surfaces: &Query<&crate::setup::TuiSurface>,
    events: &mut MessageWriter<TerminalEvent>,
) {
    if button == focus_button && *old_focus != Some(new_focus) {
        if let Some(old_entity) = *old_focus {
            events.write(TerminalEvent {
                target: remap_to_tui(old_entity, surfaces),
                input: InputEvent::FocusLost,
            });
        }

        *old_focus = Some(new_focus);

        events.write(TerminalEvent {
            target: remap_to_tui(new_focus, surfaces),
            input: InputEvent::FocusGained,
        });
    }
}

#[cfg(feature = "mouse_input")]
#[allow(clippy::too_many_arguments)]
fn emit_button_events(
    surface_entity: Entity,
    col: u16,
    row: u16,
    buttons: &ButtonInput<MouseButton>,
    touches: &Touches,
    modifiers: KeyModifiers,
    focus: &mut TerminalFocus,
    config: &TerminalInputConfig,
    surfaces: &Query<&crate::setup::TuiSurface>,
    events: &mut MessageWriter<TerminalEvent>,
) {
    let target = remap_to_tui(surface_entity, surfaces);
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        // Touch taps emulate the left mouse button: winit never synthesizes
        // mouse events from touches, so without this a tap hit-tests (via
        // the CursorPosition touch fallback) but never presses anything.
        let touch = button == MouseButton::Left;
        if buttons.just_pressed(button) || (touch && touches.any_just_pressed()) {
            emit_focus_events(
                surface_entity,
                &mut focus.focused,
                config.focus_button,
                button,
                surfaces,
                events,
            );

            events.write(TerminalEvent {
                target,
                input: InputEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(button),
                    column: col,
                    row,
                    modifiers,
                }),
            });
        }

        if buttons.just_released(button) || (touch && touches.any_just_released()) {
            events.write(TerminalEvent {
                target,
                input: InputEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(button),
                    column: col,
                    row,
                    modifiers,
                }),
            });
        }
    }
}

/// Emits one `Scroll*` event per `MouseWheel` message at the given hit
/// cell. Shared by all three `mouse_input_system` variants.
#[cfg(feature = "mouse_input")]
fn emit_scroll_events(
    target: Entity,
    col: u16,
    row: u16,
    wheel_messages: &[MouseWheel],
    modifiers: KeyModifiers,
    events: &mut MessageWriter<TerminalEvent>,
) {
    for wheel in wheel_messages {
        let Some(kind) = scroll_kind(wheel.x, wheel.y) else {
            continue;
        };
        events.write(TerminalEvent {
            target,
            input: InputEvent::Mouse(MouseEvent {
                kind,
                column: col,
                row,
                modifiers,
            }),
        });
    }
}

/// Unified mouse input system with automatic 2D/3D detection.
///
/// This system handles mouse input for both 2D UI terminals and 3D mesh terminals
/// by auto-detecting the terminal type from components and dispatching to the
/// appropriate hit-testing logic.
///
/// Terminals can be:
/// - 2D UI: Has `Node` or `ComputedNode` component (uses bounding box hit-testing)
/// - 3D Mesh: Has `Mesh2d` or `Mesh3d` component (uses ray-mesh intersection)
///
/// For hybrid entities with both mesh and node components, 3D takes priority.
///
/// The system:
/// 1. Iterates all terminals with `TerminalInput`
/// 2. Auto-detects terminal type from components
/// 3. Dispatches to appropriate hit-test function
/// 4. Collects hits with sort keys (Z-index for 2D, distance for 3D)
/// 5. Selects the topmost/closest terminal
/// 6. Emits mouse events and handles focus
#[cfg(all(feature = "mouse_input", feature = "2d", feature = "3d"))]
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn mouse_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    touches: Res<Touches>,
    mut wheel: MessageReader<MouseWheel>,
    cursor: Res<CursorPosition>,
    config: Res<TerminalInputConfig>,
    mut focus: ResMut<TerminalFocus>,
    _windows: Query<&bevy::window::Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    meshes: Res<Assets<bevy::mesh::Mesh>>,
    terminals: Query<(
        Entity,
        &TerminalInput,
        // `GlobalTransform` is required for 3D mesh ray-casting but is NOT
        // among `Node`'s required components - pure-UI entities don't have
        // it, hence Option here (see `UiGlobalTransform` below for those).
        Option<&GlobalTransform>,
        Option<&Mesh2d>,
        Option<&Mesh3d>,
        Option<&bevy::ui::Node>,
        Option<&bevy::ui::ComputedNode>,
        Option<&bevy::ui::UiGlobalTransform>,
        Option<&crate::bevy_plugin::TerminalDimensions>,
        Option<&bevy::ui::ZIndex>,
        Option<&bevy::camera::visibility::ViewVisibility>,
        Option<&bevy::camera::primitives::Aabb>,
    )>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
    // Change-detection gate (IMPROVEMENT.md D1): anything that can move a
    // cursor→cell mapping without the cursor pixel position itself
    // changing. Bundled into one `SystemParam` - see `MouseChangeProbes`.
    change_probes: MouseChangeProbes,
    mut last_cursor_pos: Local<Option<Vec2>>,
    mut last_hovered: Local<Option<(Entity, u16, u16)>>,
) {
    let wheel_messages: Vec<MouseWheel> = wheel.read().copied().collect();

    let cursor_pos = match cursor.position {
        Some(pos) => pos,
        None => {
            *last_cursor_pos = None;
            *last_hovered = None;
            return;
        }
    };

    let cursor_moved = *last_cursor_pos != Some(cursor_pos);
    let button_transition = buttons.get_just_pressed().len() > 0
        || buttons.get_just_released().len() > 0
        || touches.any_just_pressed()
        || touches.any_just_released();
    let scene_changed = change_probes.any_changed();

    if !cursor_moved && !button_transition && !scene_changed && wheel_messages.is_empty() {
        return;
    }
    *last_cursor_pos = Some(cursor_pos);
    let modifiers = read_modifiers(&keyboard);

    // Build one ray per active camera whose viewport contains the cursor,
    // topmost-rendered camera first (highest `Camera::order`). Multi-camera
    // setups (world + UI overlay) are common; a terminal is picked through
    // the frontmost camera that sees it.
    //
    // `Camera::viewport_to_world` uses the camera's real projection matrix,
    // so every projection kind (perspective, orthographic with any
    // `ScalingMode`, custom) is handled correctly.
    let mut cameras: Vec<(&Camera, &GlobalTransform)> = camera_query
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .collect();
    cameras.sort_by_key(|(camera, _)| std::cmp::Reverse(camera.order));
    let world_rays: Vec<crate::input::ray::Ray> = cameras
        .iter()
        .filter_map(|(camera, camera_transform)| {
            let viewport = camera.logical_viewport_rect()?;
            if !viewport.contains(cursor_pos) {
                return None;
            }
            let ray3d = camera
                .viewport_to_world(camera_transform, cursor_pos - viewport.min)
                .ok()?;
            Some(crate::input::ray::Ray::new(ray3d.origin, *ray3d.direction))
        })
        .collect();

    let mut hit_candidates: Vec<(Entity, HitTestResult, SortKey)> = Vec::new();

    for (
        entity,
        input,
        transform,
        mesh2d,
        mesh3d,
        node,
        computed,
        ui_transform,
        dimensions,
        z_index,
        view_visibility,
        aabb,
    ) in terminals.iter()
    {
        if !input.mouse {
            continue;
        }

        let terminal_type = detect_terminal_type(mesh2d, mesh3d, node);

        match terminal_type {
            TerminalType::Mesh3D => {
                // Get the inner Handle<Mesh> from either Mesh3d or Mesh2d
                let mesh_handle = mesh3d.map(|m| &m.0).or_else(|| mesh2d.map(|m| &m.0));
                let Some(transform) = transform else {
                    continue; // no GlobalTransform - can't ray-cast
                };

                // Stage 1 (coarse->fine, IMPROVEMENT.md D1): visibility -
                // bevy's own visibility system already computed this for
                // the frame, so an invisible terminal is skipped before
                // any per-camera work. Fail open (no component = not
                // pruned) rather than assume visible/invisible.
                if view_visibility.is_some_and(|v| !v.get()) {
                    continue;
                }

                // Test cameras front-to-back; the first camera whose ray hits
                // this terminal determines its hit (and camera priority).
                for (camera_priority, ray) in world_rays.iter().enumerate() {
                    // Stage 2: AABB bounding check - cheap rejection
                    // before touching any mesh vertex data.
                    if !ray_intersects_aabb(ray, transform, aabb) {
                        continue;
                    }

                    // Stage 3: precise triangle-level intersection.
                    if let Some((hit_result, distance)) = mesh_handle.and_then(|handle| {
                        ray_cast_hit_test_inner(ray, transform, handle, &meshes, dimensions)
                    }) {
                        hit_candidates.push((
                            entity,
                            hit_result,
                            SortKey::Distance {
                                camera_priority,
                                distance,
                            },
                        ));
                        break;
                    }
                }
            }
            TerminalType::UI2D => {
                if let Some(hit_result) =
                    bounding_box_hit_test(cursor_pos, ui_transform, node, computed, dimensions)
                {
                    let z = z_index.map(|z| z.0).unwrap_or(0);
                    hit_candidates.push((entity, hit_result, SortKey::ZIndex(z)));
                }
            }
            TerminalType::Unknown => {
                // Terminal has TerminalInput but no recognized display component
                // This shouldn't happen in normal usage, so we skip it
            }
        }
    }

    if hit_candidates.is_empty() {
        *last_hovered = None;
        return;
    }

    // Debug: Log all hits before sorting
    if hit_candidates.len() > 1 {
        debug!(
            "Multiple terminals hit at cursor ({:.1}, {:.1}):",
            cursor_pos.x, cursor_pos.y
        );
        for (entity, result, sort_key) in &hit_candidates {
            debug!(
                "  Entity {:?}: col={}, row={}, sort_key={:?}",
                entity, result.col, result.row, sort_key
            );
        }
    }

    // Sort by the custom PartialOrd which puts higher Z-index first (for 2D)
    // and closer distance first (for 3D)
    hit_candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Debug: Log selection after sorting
    if hit_candidates.len() > 1 {
        debug!(
            "After sorting, selected: Entity {:?} with sort_key={:?}",
            hit_candidates[0].0, hit_candidates[0].2
        );
    }

    if let Some((entity, hit_result, _sort_key)) = hit_candidates.first() {
        // Dedupe MouseMove (IMPROVEMENT.md D1): only emit when the
        // hovered (entity, col, row) actually changed since the last
        // recompute, so hovering inside one cell stops re-emitting on
        // every gate-triggered recompute.
        let hovered = (*entity, hit_result.col, hit_result.row);
        if *last_hovered != Some(hovered) {
            emit_mouse_move(
                *entity,
                hit_result.col,
                hit_result.row,
                &buttons,
                &touches,
                modifiers,
                &surfaces,
                &mut events,
            );
            *last_hovered = Some(hovered);
        }
        emit_button_events(
            *entity,
            hit_result.col,
            hit_result.row,
            &buttons,
            &touches,
            modifiers,
            &mut focus,
            &config,
            &surfaces,
            &mut events,
        );
        emit_scroll_events(
            remap_to_tui(*entity, &surfaces),
            hit_result.col,
            hit_result.row,
            &wheel_messages,
            modifiers,
            &mut events,
        );
    }
}

/// Mouse input for 2D UI terminals only (`3d` feature disabled - no mesh
/// terminals can exist, so there is nothing to auto-detect).
#[cfg(all(feature = "mouse_input", feature = "2d", not(feature = "3d")))]
#[allow(clippy::type_complexity)]
pub fn mouse_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    touches: Res<Touches>,
    mut wheel: MessageReader<MouseWheel>,
    cursor: Res<CursorPosition>,
    config: Res<TerminalInputConfig>,
    mut focus: ResMut<TerminalFocus>,
    terminals: Query<(
        Entity,
        &TerminalInput,
        Option<&bevy::ui::Node>,
        Option<&bevy::ui::ComputedNode>,
        Option<&bevy::ui::UiGlobalTransform>,
        Option<&crate::bevy_plugin::TerminalDimensions>,
        Option<&bevy::ui::ZIndex>,
    )>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
    // Change-detection gate (IMPROVEMENT.md D1) - no camera/3D probe
    // needed here: `bounding_box_hit_test` is pure screen-space, it never
    // reads a camera.
    terminal_ui_changed: Query<
        (),
        (
            With<TerminalInput>,
            Or<(Changed<bevy::ui::ComputedNode>, Changed<bevy::ui::UiGlobalTransform>)>,
        ),
    >,
    mut last_cursor_pos: Local<Option<Vec2>>,
    mut last_hovered: Local<Option<(Entity, u16, u16)>>,
) {
    let wheel_messages: Vec<MouseWheel> = wheel.read().copied().collect();

    let cursor_pos = match cursor.position {
        Some(pos) => pos,
        None => {
            *last_cursor_pos = None;
            *last_hovered = None;
            return;
        }
    };

    let cursor_moved = *last_cursor_pos != Some(cursor_pos);
    let button_transition = buttons.get_just_pressed().len() > 0
        || buttons.get_just_released().len() > 0
        || touches.any_just_pressed()
        || touches.any_just_released();
    if !cursor_moved && !button_transition && terminal_ui_changed.is_empty() && wheel_messages.is_empty() {
        return;
    }
    *last_cursor_pos = Some(cursor_pos);
    let modifiers = read_modifiers(&keyboard);

    let mut hit_candidates: Vec<(Entity, HitTestResult, SortKey)> = Vec::new();

    for (entity, input, node, computed, ui_transform, dimensions, z_index) in terminals.iter() {
        if !input.mouse {
            continue;
        }

        if let Some(hit_result) =
            bounding_box_hit_test(cursor_pos, ui_transform, node, computed, dimensions)
        {
            let z = z_index.map(|z| z.0).unwrap_or(0);
            hit_candidates.push((entity, hit_result, SortKey::ZIndex(z)));
        }
    }

    if hit_candidates.is_empty() {
        *last_hovered = None;
        return;
    }

    hit_candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((entity, hit_result, _sort_key)) = hit_candidates.first() {
        let hovered = (*entity, hit_result.col, hit_result.row);
        if *last_hovered != Some(hovered) {
            emit_mouse_move(
                *entity,
                hit_result.col,
                hit_result.row,
                &buttons,
                &touches,
                modifiers,
                &surfaces,
                &mut events,
            );
            *last_hovered = Some(hovered);
        }
        emit_button_events(
            *entity,
            hit_result.col,
            hit_result.row,
            &buttons,
            &touches,
            modifiers,
            &mut focus,
            &config,
            &surfaces,
            &mut events,
        );
        emit_scroll_events(
            remap_to_tui(*entity, &surfaces),
            hit_result.col,
            hit_result.row,
            &wheel_messages,
            modifiers,
            &mut events,
        );
    }
}

/// Mouse input for 3D mesh terminals only (`2d` feature disabled - no UI
/// terminals can exist, so there is nothing to auto-detect).
#[cfg(all(feature = "mouse_input", feature = "3d", not(feature = "2d")))]
#[allow(clippy::type_complexity)]
pub fn mouse_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    touches: Res<Touches>,
    mut wheel: MessageReader<MouseWheel>,
    cursor: Res<CursorPosition>,
    config: Res<TerminalInputConfig>,
    mut focus: ResMut<TerminalFocus>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    meshes: Res<Assets<bevy::mesh::Mesh>>,
    terminals: Query<(
        Entity,
        &TerminalInput,
        Option<&GlobalTransform>,
        Option<&Mesh2d>,
        Option<&Mesh3d>,
        Option<&crate::bevy_plugin::TerminalDimensions>,
        Option<&bevy::camera::visibility::ViewVisibility>,
        Option<&bevy::camera::primitives::Aabb>,
    )>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
    // Change-detection gate (IMPROVEMENT.md D1) - see the unified system's
    // doc comment for the full rationale; no UI-layout probe needed here
    // since there is no 2D UI terminal kind in this build.
    camera_change_probe: Query<
        (),
        (
            With<Camera>,
            Or<(Changed<GlobalTransform>, Changed<Projection>, Changed<Camera>)>,
        ),
    >,
    terminal_3d_changed: Query<(), (With<TerminalInput>, Changed<GlobalTransform>)>,
    mut last_cursor_pos: Local<Option<Vec2>>,
    mut last_hovered: Local<Option<(Entity, u16, u16)>>,
) {
    let wheel_messages: Vec<MouseWheel> = wheel.read().copied().collect();

    let cursor_pos = match cursor.position {
        Some(pos) => pos,
        None => {
            *last_cursor_pos = None;
            *last_hovered = None;
            return;
        }
    };

    let cursor_moved = *last_cursor_pos != Some(cursor_pos);
    let button_transition = buttons.get_just_pressed().len() > 0
        || buttons.get_just_released().len() > 0
        || touches.any_just_pressed()
        || touches.any_just_released();
    let scene_changed = !camera_change_probe.is_empty() || !terminal_3d_changed.is_empty();
    if !cursor_moved && !button_transition && !scene_changed && wheel_messages.is_empty() {
        return;
    }
    *last_cursor_pos = Some(cursor_pos);
    let modifiers = read_modifiers(&keyboard);

    // See the unified system's doc comment for the multi-camera rationale -
    // identical here, just without any 2D UI terminals to also consider.
    let mut cameras: Vec<(&Camera, &GlobalTransform)> = camera_query
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .collect();
    cameras.sort_by_key(|(camera, _)| std::cmp::Reverse(camera.order));
    let world_rays: Vec<crate::input::ray::Ray> = cameras
        .iter()
        .filter_map(|(camera, camera_transform)| {
            let viewport = camera.logical_viewport_rect()?;
            if !viewport.contains(cursor_pos) {
                return None;
            }
            let ray3d = camera
                .viewport_to_world(camera_transform, cursor_pos - viewport.min)
                .ok()?;
            Some(crate::input::ray::Ray::new(ray3d.origin, *ray3d.direction))
        })
        .collect();

    let mut hit_candidates: Vec<(Entity, HitTestResult, SortKey)> = Vec::new();

    for (entity, input, transform, mesh2d, mesh3d, dimensions, view_visibility, aabb) in
        terminals.iter()
    {
        if !input.mouse {
            continue;
        }
        let mesh_handle = mesh3d.map(|m| &m.0).or_else(|| mesh2d.map(|m| &m.0));
        let Some(transform) = transform else {
            continue; // no GlobalTransform - can't ray-cast
        };

        // Stage 1 (coarse->fine, IMPROVEMENT.md D1): visibility. Fail
        // open (no component = not pruned).
        if view_visibility.is_some_and(|v| !v.get()) {
            continue;
        }

        for (camera_priority, ray) in world_rays.iter().enumerate() {
            // Stage 2: AABB bounding check.
            if !ray_intersects_aabb(ray, transform, aabb) {
                continue;
            }

            // Stage 3: precise triangle-level intersection.
            if let Some((hit_result, distance)) = mesh_handle.and_then(|handle| {
                ray_cast_hit_test_inner(ray, transform, handle, &meshes, dimensions)
            }) {
                hit_candidates.push((
                    entity,
                    hit_result,
                    SortKey::Distance {
                        camera_priority,
                        distance,
                    },
                ));
                break;
            }
        }
    }

    if hit_candidates.is_empty() {
        *last_hovered = None;
        return;
    }

    hit_candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((entity, hit_result, _sort_key)) = hit_candidates.first() {
        let hovered = (*entity, hit_result.col, hit_result.row);
        if *last_hovered != Some(hovered) {
            emit_mouse_move(
                *entity,
                hit_result.col,
                hit_result.row,
                &buttons,
                &touches,
                modifiers,
                &surfaces,
                &mut events,
            );
            *last_hovered = Some(hovered);
        }
        emit_button_events(
            *entity,
            hit_result.col,
            hit_result.row,
            &buttons,
            &touches,
            modifiers,
            &mut focus,
            &config,
            &surfaces,
            &mut events,
        );
        emit_scroll_events(
            remap_to_tui(*entity, &surfaces),
            hit_result.col,
            hit_result.row,
            &wheel_messages,
            modifiers,
            &mut events,
        );
    }
}

/// Window resize event system.
///
/// Listens for window resize events and forwards them to all terminals.
pub fn window_resize_system(
    mut resize_events: MessageReader<bevy::window::WindowResized>,
    terminals: Query<Entity, With<crate::setup::Tui>>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
) {
    for resize_event in resize_events.read() {
        let pixels = UVec2::new(resize_event.width as u32, resize_event.height as u32);

        // Emit resize event for all terminals
        for entity in terminals.iter() {
            events.write(TerminalEvent {
                target: remap_to_tui(entity, &surfaces),
                input: InputEvent::Resize { pixels },
            });
        }
    }
}

/// Terminal focus cycling system.
///
/// Handles Tab key to cycle focus between terminals with `TerminalInput` component.
/// Emits FocusGained/FocusLost events when focus changes.
pub fn terminal_focus_system(
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    mut focus: ResMut<TerminalFocus>,
    terminals: Query<(Entity, &TerminalInput)>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
) {
    // Check if Tab was just pressed
    if !keyboard.just_pressed(BevyKeyCode::Tab) {
        return;
    }

    // Collect terminals with keyboard input enabled
    let mut terminal_entities: Vec<Entity> = terminals
        .iter()
        .filter(|(_, input)| input.keyboard)
        .map(|(entity, _)| entity)
        .collect();

    if terminal_entities.is_empty() {
        return;
    }

    // Sort for consistent ordering
    terminal_entities.sort();

    // Find current focus index
    let current_index = focus.focused.and_then(|focused| {
        terminal_entities
            .iter()
            .position(|&entity| entity == focused)
    });

    // Calculate next index (wrap around)
    let next_index = match current_index {
        Some(idx) => (idx + 1) % terminal_entities.len(),
        None => 0, // No focus, start at first terminal
    };

    let next_entity = terminal_entities[next_index];

    // Update focus if changed
    if focus.focused != Some(next_entity) {
        // Emit FocusLost for old focus
        if let Some(old_focus) = focus.focused {
            events.write(TerminalEvent {
                target: remap_to_tui(old_focus, &surfaces),
                input: InputEvent::FocusLost,
            });
        }

        // Update focus
        focus.focused = Some(next_entity);

        // Emit FocusGained
        events.write(TerminalEvent {
            target: remap_to_tui(next_entity, &surfaces),
            input: InputEvent::FocusGained,
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keycode_from_logical_character() {
        assert_eq!(
            keycode_from_logical(&Key::Character("a".into()), false, BevyKeyCode::KeyA),
            Some(KeyCode::Char('a'))
        );
        assert_eq!(
            keycode_from_logical(&Key::Character("Ω".into()), false, BevyKeyCode::KeyO),
            Some(KeyCode::Char('Ω'))
        );
    }

    #[test]
    fn test_keycode_from_logical_empty_character_is_none() {
        assert_eq!(
            keycode_from_logical(&Key::Character("".into()), false, BevyKeyCode::KeyA),
            None
        );
    }

    #[test]
    fn test_keycode_from_logical_tab_and_backtab() {
        assert_eq!(
            keycode_from_logical(&Key::Tab, false, BevyKeyCode::Tab),
            Some(KeyCode::Tab)
        );
        assert_eq!(
            keycode_from_logical(&Key::Tab, true, BevyKeyCode::Tab),
            Some(KeyCode::BackTab)
        );
    }

    #[test]
    fn test_keycode_from_logical_function_key() {
        assert_eq!(
            keycode_from_logical(&Key::F5, false, BevyKeyCode::F5),
            Some(KeyCode::F(5))
        );
    }

    #[test]
    fn test_keycode_from_logical_unknown_falls_back_to_physical() {
        assert_eq!(
            keycode_from_logical(&Key::Alt, false, BevyKeyCode::AltLeft),
            Some(KeyCode::Unidentified(BevyKeyCode::AltLeft))
        );
    }

    #[test]
    fn test_key_modifiers_default() {
        let modifiers = KeyModifiers::default();
        assert!(!modifiers.ctrl);
        assert!(!modifiers.alt);
        assert!(!modifiers.shift);
        assert!(!modifiers.meta);
    }

    #[test]
    fn test_terminal_input_default() {
        let input = TerminalInput::default();
        assert!(input.keyboard);
        assert!(input.mouse);
    }

    #[test]
    fn test_terminal_input_config_default() {
        let config = TerminalInputConfig::default();
        assert!(config.keyboard_enabled);
        assert!(config.mouse_enabled);
        assert!(config.auto_focus);
        assert_eq!(config.focus_button, MouseButton::Left);
    }

    #[test]
    fn test_read_modifiers() {
        let mut input = ButtonInput::<BevyKeyCode>::default();
        input.press(BevyKeyCode::ControlLeft);
        let modifiers = read_modifiers(&input);
        assert!(modifiers.ctrl);
        assert!(!modifiers.alt);
        assert!(!modifiers.shift);
        assert!(!modifiers.meta);

        let empty = ButtonInput::<BevyKeyCode>::default();
        let modifiers = read_modifiers(&empty);
        assert!(!modifiers.ctrl);
        assert!(!modifiers.alt);
        assert!(!modifiers.shift);
        assert!(!modifiers.meta);
    }

    #[cfg(feature = "mouse_input")]
    mod move_kind_tests {
        use super::super::move_kind;
        use bevy::input::mouse::MouseButton;
        use crate::input::MouseEventKind;

        #[test]
        fn no_buttons_is_moved() {
            assert_eq!(move_kind(false, false, false), MouseEventKind::Moved);
        }

        #[test]
        fn left_button_drags() {
            assert_eq!(
                move_kind(true, false, false),
                MouseEventKind::Drag(MouseButton::Left)
            );
        }

        #[test]
        fn left_wins_over_right_when_both_held() {
            assert_eq!(
                move_kind(true, true, false),
                MouseEventKind::Drag(MouseButton::Left)
            );
        }
    }

    #[cfg(feature = "mouse_input")]
    mod scroll_kind_tests {
        use super::super::scroll_kind;
        use crate::input::MouseEventKind;

        #[test]
        fn positive_y_scrolls_up() {
            assert_eq!(scroll_kind(0.0, 1.0), Some(MouseEventKind::ScrollUp));
        }

        #[test]
        fn negative_y_scrolls_down() {
            assert_eq!(scroll_kind(0.0, -1.0), Some(MouseEventKind::ScrollDown));
        }

        #[test]
        fn y_takes_priority_over_x() {
            assert_eq!(scroll_kind(1.0, 1.0), Some(MouseEventKind::ScrollUp));
        }

        #[test]
        fn positive_x_scrolls_right_when_y_is_zero() {
            assert_eq!(scroll_kind(1.0, 0.0), Some(MouseEventKind::ScrollRight));
        }

        #[test]
        fn all_zero_is_none() {
            assert_eq!(scroll_kind(0.0, 0.0), None);
        }
    }

    // ========================================================================
    // Coordinate mapping (P2-4): pixel_to_cell (2D UI) / uv_to_cell (3D mesh).
    // Pure math, no bevy types - exercised directly.
    // ========================================================================

    #[cfg(all(feature = "mouse_input", feature = "2d"))]
    mod pixel_to_cell_tests {
        use super::super::pixel_to_cell;

        #[test]
        fn origin_maps_to_first_cell() {
            assert_eq!(pixel_to_cell(0.0, 0.0, 8.0, 16.0, 80, 24), (0, 0));
        }

        #[test]
        fn exactly_on_a_cell_boundary_rounds_down_into_the_next_cell() {
            // x=8.0 is exactly the boundary between column 0 and column 1 -
            // ratatui/terminal convention: the boundary pixel belongs to the
            // cell it starts (column 1), not the one it ends (column 0).
            assert_eq!(pixel_to_cell(8.0, 16.0, 8.0, 16.0, 80, 24), (1, 1));
            // Just before the boundary still belongs to the previous cell.
            assert_eq!(pixel_to_cell(7.9, 15.9, 8.0, 16.0, 80, 24), (0, 0));
        }

        #[test]
        fn last_column_and_row_clamp_correctly() {
            // Exactly at the terminal's pixel edge (80 cols * 8px = 640,
            // 24 rows * 16px = 384) must clamp to the LAST valid index
            // (79, 23), not 80/24 (one past the grid).
            assert_eq!(pixel_to_cell(640.0, 384.0, 8.0, 16.0, 80, 24), (79, 23));
            // Comfortably inside the last cell must resolve the same way.
            assert_eq!(pixel_to_cell(635.0, 380.0, 8.0, 16.0, 80, 24), (79, 23));
        }

        #[test]
        fn out_of_bounds_pixels_clamp_instead_of_wrapping_or_panicking() {
            assert_eq!(
                pixel_to_cell(-50.0, -50.0, 8.0, 16.0, 80, 24),
                (0, 0),
                "negative input must clamp to the first cell"
            );
            assert_eq!(
                pixel_to_cell(10_000.0, 10_000.0, 8.0, 16.0, 80, 24),
                (79, 23),
                "far-out-of-bounds input must clamp to the last cell"
            );
        }

        #[test]
        fn single_cell_terminal_always_resolves_to_zero() {
            assert_eq!(pixel_to_cell(500.0, 500.0, 8.0, 16.0, 1, 1), (0, 0));
        }
    }

    #[cfg(all(feature = "mouse_input", feature = "3d"))]
    mod uv_to_cell_tests {
        use super::super::uv_to_cell;

        #[test]
        fn origin_maps_to_first_cell() {
            assert_eq!(uv_to_cell(0.0, 0.0, 80, 24), (0, 0));
        }

        #[test]
        fn uv_one_clamps_to_the_last_cell_not_one_past_it() {
            // The bug this guards against: a naive `(uv * cols).clamp(0.0,
            // cols)` allows exactly `cols` at `uv == 1.0`, one past the last
            // valid grid index (0..cols-1).
            assert_eq!(uv_to_cell(1.0, 1.0, 80, 24), (79, 23));
        }

        #[test]
        fn slightly_past_one_still_clamps_to_the_last_cell() {
            // Ray-mesh intersection can return UV fractionally outside
            // 0.0..=1.0 at triangle edges due to floating-point error.
            assert_eq!(uv_to_cell(1.0001, 1.0001, 80, 24), (79, 23));
        }

        #[test]
        fn negative_uv_clamps_to_the_first_cell() {
            assert_eq!(uv_to_cell(-0.0001, -0.0001, 80, 24), (0, 0));
        }

        #[test]
        fn midpoint_maps_to_the_middle_of_the_grid() {
            assert_eq!(uv_to_cell(0.5, 0.5, 80, 24), (40, 12));
        }
    }
}
