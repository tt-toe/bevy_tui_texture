//! Event-driven terminal input via Bevy's ECS.
//!
//! Bevy input systems → `TerminalEvent` messages → User systems → Terminal updates

use bevy::prelude::*;
use tracing::debug;
//use bevy::log::debug;
//use log::debug;

// Ray casting for 3D mouse input
#[cfg(feature = "mouse_input")]
pub mod ray;

// ============================================================================
// Events
// ============================================================================

/// Message emitted for terminal input.
///
/// These messages are emitted by input capture systems and read by user systems
/// to handle terminal input. Messages are entity-targeted, enabling selective
/// routing to specific terminal instances.
#[derive(Message, Clone, Debug)]
pub struct TerminalEvent {
    /// The terminal entity that should receive this message
    pub target: Entity,
    /// The event payload
    pub event: TerminalEventType,
}

/// Types of terminal events.
#[derive(Clone, Debug)]
pub enum TerminalEventType {
    /// Keyboard key was pressed.
    ///
    /// Emitted for any key press, including modifier keys.
    /// Check `modifiers` field for Ctrl, Alt, Shift, Meta state.
    KeyPress {
        key: KeyCode,
        modifiers: KeyModifiers,
    },

    /// Character input for text entry.
    ///
    /// Emitted for printable characters (a-z, 0-9, punctuation, etc).
    /// This is separate from `KeyPress` to simplify text input handling.
    CharInput { character: char },

    /// Mouse button was pressed.
    ///
    /// The `position` is in terminal coordinates (row, col),
    /// not screen pixels.
    MousePress {
        button: MouseButton,
        /// Terminal coordinates (row, col)
        position: (u16, u16),
    },

    /// Mouse button was released.
    MouseRelease {
        button: MouseButton,
        /// Terminal coordinates (row, col)
        position: (u16, u16),
    },

    /// Mouse cursor moved over terminal.
    ///
    /// Only emitted when cursor is over the terminal.
    MouseMove {
        /// Terminal coordinates (row, col)
        position: (u16, u16),
    },

    /// Terminal gained input focus.
    ///
    /// Emitted when focus changes to this terminal.
    FocusGained,

    /// Terminal lost input focus.
    ///
    /// Emitted when focus changes away from this terminal.
    FocusLost,

    /// Window/terminal was resized.
    ///
    /// Emitted for window resize events. The user is responsible for
    /// recreating the terminal backend with new dimensions if needed.
    Resize { new_size: (u32, u32) },
}

/// Modifier keys state.
#[derive(Clone, Debug, Default)]
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
/// for hit-testing.
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

/// Convert KeyCode to character with shift. Returns `None` for non-printable keys.
pub fn keycode_to_char(key: KeyCode, shift: bool) -> Option<char> {
    use KeyCode::*;

    match key {
        // Letters
        KeyA => Some(if shift { 'A' } else { 'a' }),
        KeyB => Some(if shift { 'B' } else { 'b' }),
        KeyC => Some(if shift { 'C' } else { 'c' }),
        KeyD => Some(if shift { 'D' } else { 'd' }),
        KeyE => Some(if shift { 'E' } else { 'e' }),
        KeyF => Some(if shift { 'F' } else { 'f' }),
        KeyG => Some(if shift { 'G' } else { 'g' }),
        KeyH => Some(if shift { 'H' } else { 'h' }),
        KeyI => Some(if shift { 'I' } else { 'i' }),
        KeyJ => Some(if shift { 'J' } else { 'j' }),
        KeyK => Some(if shift { 'K' } else { 'k' }),
        KeyL => Some(if shift { 'L' } else { 'l' }),
        KeyM => Some(if shift { 'M' } else { 'm' }),
        KeyN => Some(if shift { 'N' } else { 'n' }),
        KeyO => Some(if shift { 'O' } else { 'o' }),
        KeyP => Some(if shift { 'P' } else { 'p' }),
        KeyQ => Some(if shift { 'Q' } else { 'q' }),
        KeyR => Some(if shift { 'R' } else { 'r' }),
        KeyS => Some(if shift { 'S' } else { 's' }),
        KeyT => Some(if shift { 'T' } else { 't' }),
        KeyU => Some(if shift { 'U' } else { 'u' }),
        KeyV => Some(if shift { 'V' } else { 'v' }),
        KeyW => Some(if shift { 'W' } else { 'w' }),
        KeyX => Some(if shift { 'X' } else { 'x' }),
        KeyY => Some(if shift { 'Y' } else { 'y' }),
        KeyZ => Some(if shift { 'Z' } else { 'z' }),

        // Numbers and shifted symbols
        Digit1 => Some(if shift { '!' } else { '1' }),
        Digit2 => Some(if shift { '@' } else { '2' }),
        Digit3 => Some(if shift { '#' } else { '3' }),
        Digit4 => Some(if shift { '$' } else { '4' }),
        Digit5 => Some(if shift { '%' } else { '5' }),
        Digit6 => Some(if shift { '^' } else { '6' }),
        Digit7 => Some(if shift { '&' } else { '7' }),
        Digit8 => Some(if shift { '*' } else { '8' }),
        Digit9 => Some(if shift { '(' } else { '9' }),
        Digit0 => Some(if shift { ')' } else { '0' }),

        // Punctuation
        Space => Some(' '),
        Minus => Some(if shift { '_' } else { '-' }),
        Equal => Some(if shift { '+' } else { '=' }),
        BracketLeft => Some(if shift { '{' } else { '[' }),
        BracketRight => Some(if shift { '}' } else { ']' }),
        Backslash => Some(if shift { '|' } else { '\\' }),
        Semicolon => Some(if shift { ':' } else { ';' }),
        Quote => Some(if shift { '"' } else { '\'' }),
        Comma => Some(if shift { '<' } else { ',' }),
        Period => Some(if shift { '>' } else { '.' }),
        Slash => Some(if shift { '?' } else { '/' }),
        Backquote => Some(if shift { '~' } else { '`' }),

        // Non-printable keys
        _ => None,
    }
}

// ============================================================================
// Input Systems
// ============================================================================

/// Update cursor position from window.
///
/// This system reads the cursor position from the primary window and updates
/// the `CursorPosition` resource for use by other input systems.
pub fn update_cursor_position_system(
    mut cursor_pos: ResMut<CursorPosition>,
    windows: Query<&Window>,
) {
    // Get primary window
    if let Ok(window) = windows.single() {
        cursor_pos.position = window.cursor_position();
    }
}

/// Keyboard input capture system.
///
/// Captures keyboard input and emits `TerminalEvent`s for the focused terminal.
/// Only processes input if a terminal has focus and has keyboard input enabled.
pub fn keyboard_input_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<TerminalFocus>,
    terminals: Query<&TerminalInput>,
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

    // Check for modifier keys
    let modifiers = KeyModifiers {
        ctrl: keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight),
        alt: keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight),
        shift: keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight),
        meta: keyboard.pressed(KeyCode::SuperLeft) || keyboard.pressed(KeyCode::SuperRight),
    };

    // Process all just-pressed keys
    for key in keyboard.get_just_pressed() {
        // Emit KeyPress event
        events.write(TerminalEvent {
            target: focused_entity,
            event: TerminalEventType::KeyPress {
                key: *key,
                modifiers: modifiers.clone(),
            },
        });

        // Emit CharInput for printable characters
        if let Some(character) = keycode_to_char(*key, modifiers.shift) {
            events.write(TerminalEvent {
                target: focused_entity,
                event: TerminalEventType::CharInput { character },
            });
        }
    }
}

// ============================================================================
// Mouse Input - Unified System Helpers
// ============================================================================

/// Terminal type detected from components.
#[cfg(feature = "mouse_input")]
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
    /// Distance for 3D mesh terminals (lower = closer)
    Distance(f32),
}

#[cfg(feature = "mouse_input")]
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (SortKey::ZIndex(a), SortKey::ZIndex(b)) => b.partial_cmp(a), // Higher Z on top
            (SortKey::Distance(a), SortKey::Distance(b)) => a.partial_cmp(b), // Closer first
            _ => None, // Can't compare ZIndex with Distance
        }
    }
}

/// Detect terminal type from components.
///
/// Priority: 3D mesh > 2D UI (for hybrid entities)
#[cfg(feature = "mouse_input")]
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

/// Perform 2D UI bounding box hit test.
///
/// Converts cursor position to terminal grid coordinates using UI layout bounds.
#[cfg(feature = "mouse_input")]
fn bounding_box_hit_test(
    cursor_pos: bevy::math::Vec2,
    transform: Option<&bevy::transform::components::GlobalTransform>,
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
    } else if let Some(n) = node {
        match (n.width, n.height) {
            (bevy::ui::Val::Px(w), bevy::ui::Val::Px(h)) => (w, h, 1.0),
            _ => return None, // Not in pixels
        }
    } else {
        return None; // No size info
    };

    // Coordinate system handling for Bevy UI:
    // - Cursor (input) is in UI coordinates: top-left origin, +Y down, pixels
    // - GlobalTransform is in world coordinates: center origin, +Y up
    // - Node.left/top are in UI coordinates: top-left origin, +Y down, pixels
    //
    // For UI nodes, we need to use UI coordinate system to match cursor position!

    // Debug: Log all available position information
    if let Some(t) = transform {
        let gt = t.translation();
        debug!(
            "GlobalTransform (world coords, center origin): ({:.1}, {:.1}, {:.1})",
            gt.x, gt.y, gt.z
        );
    }
    if let Some(n) = node {
        debug!(
            "Node (UI coords, top-left origin): left={:?}, top={:?}, position_type={:?}",
            n.left, n.top, n.position_type
        );
    }
    if let Some(c) = computed {
        debug!(
            "ComputedNode: physical_size=({:.1}, {:.1}), inverse_scale={:.2} → logical_size=({:.1}, {:.1})",
            c.unrounded_size().x,
            c.unrounded_size().y,
            inverse_scale,
            width_px,
            height_px
        );
    }

    // Get position in UI coordinates (top-left origin, +Y down)
    // For UI nodes, use Node.left/top which are in logical pixels
    let (node_min_x, node_min_y) = if let Some(n) = node {
        let left_px = match n.left {
            bevy::ui::Val::Px(v) => v,
            bevy::ui::Val::Auto => 0.0,
            bevy::ui::Val::Percent(_) => 0.0,
            _ => 0.0,
        };
        let top_px = match n.top {
            bevy::ui::Val::Px(v) => v,
            bevy::ui::Val::Auto => 0.0,
            bevy::ui::Val::Percent(_) => 0.0,
            _ => 0.0,
        };

        debug!(
            "Hit test using Node: pos=({:.1}, {:.1}), size=({:.1}x{:.1}) (all in logical pixels)",
            left_px, top_px, width_px, height_px
        );

        (left_px, top_px)
    } else {
        debug!(
            "Hit test using fallback: origin with size=({:.1}x{:.1})",
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
    let col = (local_x / char_width).min(cols - 1.0) as u16;
    let row = (local_y / char_height).min(rows - 1.0) as u16;

    debug!("Hit test result: col={}, row={}", col, row);

    Some(HitTestResult { col, row })
}

/// Perform 3D ray-mesh hit test.
///
/// Converts cursor position to terminal grid coordinates using ray casting and UV mapping.
/// Works with both Mesh2d and Mesh3d by accepting the inner Handle<Mesh>.
#[cfg(feature = "mouse_input")]
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
        (dims.cols as f32, dims.rows as f32)
    } else {
        (80.0, 24.0)
    };

    // UV to terminal grid mapping (90° CCW rotated mesh)
    let col = (uv.x * cols).clamp(0.0, cols) as u16;
    let row = (uv.y * rows).clamp(0.0, rows) as u16;

    debug!(
        "3D Hit Test: uv=({:.3},{:.3}) distance={:.1} cols={} rows={} -> grid=({},{})",
        uv.x, uv.y, hit.distance, cols, rows, col, row
    );

    Some((HitTestResult { col, row }, hit.distance))
}

#[cfg(feature = "mouse_input")]
fn emit_mouse_move(entity: Entity, col: u16, row: u16, events: &mut MessageWriter<TerminalEvent>) {
    events.write(TerminalEvent {
        target: entity,
        event: TerminalEventType::MouseMove {
            position: (col, row),
        },
    });
}

#[cfg(feature = "mouse_input")]
fn emit_focus_events(
    new_focus: Entity,
    old_focus: &mut Option<Entity>,
    focus_button: MouseButton,
    button: MouseButton,
    events: &mut MessageWriter<TerminalEvent>,
) {
    if button == focus_button && *old_focus != Some(new_focus) {
        if let Some(old_entity) = *old_focus {
            events.write(TerminalEvent {
                target: old_entity,
                event: TerminalEventType::FocusLost,
            });
        }

        *old_focus = Some(new_focus);

        events.write(TerminalEvent {
            target: new_focus,
            event: TerminalEventType::FocusGained,
        });
    }
}

#[cfg(feature = "mouse_input")]
fn emit_button_events(
    entity: Entity,
    col: u16,
    row: u16,
    buttons: &ButtonInput<MouseButton>,
    focus: &mut TerminalFocus,
    config: &TerminalInputConfig,
    events: &mut MessageWriter<TerminalEvent>,
) {
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        if buttons.just_pressed(button) {
            emit_focus_events(
                entity,
                &mut focus.focused,
                config.focus_button,
                button,
                events,
            );

            events.write(TerminalEvent {
                target: entity,
                event: TerminalEventType::MousePress {
                    button,
                    position: (col, row),
                },
            });
        }

        if buttons.just_released(button) {
            events.write(TerminalEvent {
                target: entity,
                event: TerminalEventType::MouseRelease {
                    button,
                    position: (col, row),
                },
            });
        }
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
#[cfg(feature = "mouse_input")]
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn mouse_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorPosition>,
    config: Res<TerminalInputConfig>,
    mut focus: ResMut<TerminalFocus>,
    _windows: Query<&bevy::window::Window>,
    camera_query: Query<(&Camera, &GlobalTransform, &Projection)>,
    meshes: Res<Assets<bevy::mesh::Mesh>>,
    terminals: Query<(
        Entity,
        &TerminalInput,
        &GlobalTransform,
        Option<&Mesh2d>,
        Option<&Mesh3d>,
        Option<&bevy::ui::Node>,
        Option<&bevy::ui::ComputedNode>,
        Option<&crate::bevy_plugin::TerminalDimensions>,
        Option<&bevy::ui::ZIndex>,
    )>,
    mut events: MessageWriter<TerminalEvent>,
) {
    let cursor_pos = match cursor.position {
        Some(pos) => pos,
        None => return,
    };

    let world_ray: Option<crate::input::ray::Ray> = match camera_query.single() {
        Ok((camera, camera_transform, projection)) => {
            if let Some(viewport) = camera.logical_viewport_rect() {
                let cursor_ndc = Vec2::new(
                    (cursor_pos.x - viewport.min.x) / viewport.width() * 2.0 - 1.0,
                    -((cursor_pos.y - viewport.min.y) / viewport.height() * 2.0 - 1.0),
                );
                let viewport_size = Vec2::new(viewport.width(), viewport.height());
                Some(crate::input::ray::Ray::from_camera(
                    cursor_ndc,
                    camera_transform,
                    projection,
                    viewport_size,
                ))
            } else {
                None
            }
        }
        Err(_) => {
            // No camera available - continue with 2D-only processing
            None
        }
    };

    let mut hit_candidates: Vec<(Entity, HitTestResult, SortKey)> = Vec::new();

    for (entity, input, transform, mesh2d, mesh3d, node, computed, dimensions, z_index) in
        terminals.iter()
    {
        if !input.mouse {
            continue;
        }

        let terminal_type = detect_terminal_type(mesh2d, mesh3d, node);

        match terminal_type {
            TerminalType::Mesh3D => {
                // Check Mesh3d first, then fallback to Mesh2d for backward compatibility
                if let Some(ray) = world_ray.as_ref() {
                    // Get the inner Handle<Mesh> from either Mesh3d or Mesh2d
                    let mesh_handle = mesh3d.map(|m| &m.0).or_else(|| mesh2d.map(|m| &m.0));

                    if let Some((hit_result, distance)) = mesh_handle
                        .and_then(|handle| ray_cast_hit_test_inner(ray, transform, handle, &meshes, dimensions))
                    {
                        hit_candidates.push((entity, hit_result, SortKey::Distance(distance)));
                    }
                }
            }
            TerminalType::UI2D => {
                if let Some(hit_result) =
                    bounding_box_hit_test(cursor_pos, Some(transform), node, computed, dimensions)
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
        emit_mouse_move(*entity, hit_result.col, hit_result.row, &mut events);
        emit_button_events(
            *entity,
            hit_result.col,
            hit_result.row,
            &buttons,
            &mut focus,
            &config,
            &mut events,
        );
    }
}

/// Window resize event system.
///
/// Listens for window resize events and forwards them to all terminals.
pub fn window_resize_system(
    mut resize_events: MessageReader<bevy::window::WindowResized>,
    terminals: Query<Entity, With<crate::TerminalComponent>>,
    mut events: MessageWriter<TerminalEvent>,
) {
    for resize_event in resize_events.read() {
        let new_size = (resize_event.width as u32, resize_event.height as u32);

        // Emit resize event for all terminals
        for entity in terminals.iter() {
            events.write(TerminalEvent {
                target: entity,
                event: TerminalEventType::Resize { new_size },
            });
        }
    }
}

/// Terminal focus cycling system.
///
/// Handles Tab key to cycle focus between terminals with `TerminalInput` component.
/// Emits FocusGained/FocusLost events when focus changes.
pub fn terminal_focus_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<TerminalFocus>,
    terminals: Query<(Entity, &TerminalInput)>,
    mut events: MessageWriter<TerminalEvent>,
) {
    // Check if Tab was just pressed
    if !keyboard.just_pressed(KeyCode::Tab) {
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
                target: old_focus,
                event: TerminalEventType::FocusLost,
            });
        }

        // Update focus
        focus.focused = Some(next_entity);

        // Emit FocusGained
        events.write(TerminalEvent {
            target: next_entity,
            event: TerminalEventType::FocusGained,
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
    fn test_keycode_to_char_letters() {
        assert_eq!(keycode_to_char(KeyCode::KeyA, false), Some('a'));
        assert_eq!(keycode_to_char(KeyCode::KeyA, true), Some('A'));
        assert_eq!(keycode_to_char(KeyCode::KeyZ, false), Some('z'));
        assert_eq!(keycode_to_char(KeyCode::KeyZ, true), Some('Z'));
    }

    #[test]
    fn test_keycode_to_char_numbers() {
        assert_eq!(keycode_to_char(KeyCode::Digit1, false), Some('1'));
        assert_eq!(keycode_to_char(KeyCode::Digit1, true), Some('!'));
        assert_eq!(keycode_to_char(KeyCode::Digit5, false), Some('5'));
        assert_eq!(keycode_to_char(KeyCode::Digit5, true), Some('%'));
        assert_eq!(keycode_to_char(KeyCode::Digit0, false), Some('0'));
        assert_eq!(keycode_to_char(KeyCode::Digit0, true), Some(')'));
    }

    #[test]
    fn test_keycode_to_char_punctuation() {
        assert_eq!(keycode_to_char(KeyCode::Space, false), Some(' '));
        assert_eq!(keycode_to_char(KeyCode::Comma, false), Some(','));
        assert_eq!(keycode_to_char(KeyCode::Comma, true), Some('<'));
        assert_eq!(keycode_to_char(KeyCode::Period, false), Some('.'));
        assert_eq!(keycode_to_char(KeyCode::Period, true), Some('>'));
    }

    #[test]
    fn test_keycode_to_char_non_printable() {
        assert_eq!(keycode_to_char(KeyCode::F1, false), None);
        assert_eq!(keycode_to_char(KeyCode::Escape, false), None);
        assert_eq!(keycode_to_char(KeyCode::ArrowUp, false), None);
        assert_eq!(keycode_to_char(KeyCode::Enter, false), None);
        assert_eq!(keycode_to_char(KeyCode::Backspace, false), None);
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
}
