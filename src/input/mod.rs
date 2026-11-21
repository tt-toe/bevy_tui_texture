//! Event-driven terminal input via Bevy's ECS.

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

#[cfg(all(feature = "crossterm-compat", not(target_arch = "wasm32")))]
pub mod crossterm_compat;

/// Message emitted for terminal input.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct TerminalEvent {
    /// The Tui entity that should receive this message.
    pub target: Entity,
    pub input: InputEvent,
}

/// Mirror of `crossterm::event::Event`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// Never produced by the built-in capture systems (winit has no paste
    /// event).
    Paste(String),
    FocusGained,
    FocusLost,
    /// Emitted on window resize for every terminal (same trigger as
    /// today).
    Resize {
        pixels: UVec2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

/// Mirror of `crossterm::event::KeyCode` (the subset a GPU-windowed app can
/// produce), plus an escape hatch for everything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    /// Shift+Tab, following crossterm's convention (`modifiers.shift`
    /// stays true as well).
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
    /// information is lost.
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

/// Tracks which terminal currently has input focus.
#[derive(Resource, Default, Debug)]
pub struct TerminalFocus {
    /// Entity of the currently focused terminal, or None if no terminal has focus
    pub focused: Option<Entity>,
}

/// Global config for terminal input.
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
#[derive(Resource, Default, Debug)]
pub struct CursorPosition {
    /// Current cursor position, or None if cursor is outside window
    pub position: Option<Vec2>,
}

/// Enable input routing for a terminal.
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

/// Reads the four modifier keys from bevy's `ButtonInput`.
fn read_modifiers(keyboard: &ButtonInput<BevyKeyCode>) -> KeyModifiers {
    KeyModifiers {
        ctrl: keyboard.pressed(BevyKeyCode::ControlLeft)
            || keyboard.pressed(BevyKeyCode::ControlRight),
        alt: keyboard.pressed(BevyKeyCode::AltLeft) || keyboard.pressed(BevyKeyCode::AltRight),
        shift: keyboard.pressed(BevyKeyCode::ShiftLeft)
            || keyboard.pressed(BevyKeyCode::ShiftRight),
        meta: keyboard.pressed(BevyKeyCode::SuperLeft) || keyboard.pressed(BevyKeyCode::SuperRight),
    }
}

/// Maps bevy's logical key (already layout/Shift-resolved by winit) to the
/// crossterm-shaped `KeyCode`.
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

/// Remap a hit-tested / focused entity to the [`Tui`](crate::setup::Tui)
/// entity it displays, via [`TuiSurface`](crate::setup::TuiSurface).
fn remap_to_tui(entity: Entity, surfaces: &Query<&crate::setup::TuiSurface>) -> Entity {
    surfaces.get(entity).map(|s| s.tui).unwrap_or(entity)
}

/// Update cursor position from window.
pub fn update_cursor_position_system(
    mut cursor_pos: ResMut<CursorPosition>,
    primary: Query<&Window, With<bevy::window::PrimaryWindow>>,
    windows: Query<&Window>,
    touches: Res<Touches>,
) {
    let cursor = match primary.single() {
        Ok(window) => window.cursor_position(),
        Err(_) => windows.iter().find_map(Window::cursor_position),
    };
    cursor_pos.position = cursor
        .or_else(|| touches.first_pressed_position())
        .or_else(|| touches.iter_just_released().next().map(|t| t.position()));
}

/// Keyboard input capture system.
pub fn keyboard_input_system(
    mut key_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    focus: Res<TerminalFocus>,
    terminals: Query<&TerminalInput>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
) {
    let Some(focused_entity) = focus.focused else {
        return;
    };

    let Ok(input) = terminals.get(focused_entity) else {
        return;
    };

    if !input.keyboard {
        return;
    }

    let target = remap_to_tui(focused_entity, &surfaces);
    let modifiers = read_modifiers(&keyboard);

    for key_event in key_events.read() {
        let kind = match key_event.state {
            ButtonState::Released => KeyEventKind::Release,
            ButtonState::Pressed if key_event.repeat => KeyEventKind::Repeat,
            ButtonState::Pressed => KeyEventKind::Press,
        };
        let Some(code) =
            keycode_from_logical(&key_event.logical_key, modifiers.shift, key_event.key_code)
        else {
            continue;
        };

        events.write(TerminalEvent {
            target,
            input: InputEvent::Key(KeyEvent {
                code,
                modifiers,
                kind,
            }),
        });
    }
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
    #[cfg(feature = "2d")]
    ZIndex(i32),
    /// 3D mesh terminal hit: camera priority (0 = topmost-rendered camera,
    /// i.e. highest `Camera::order`), then ray distance (lower = closer).
    #[cfg(feature = "3d")]
    Distance {
        camera_priority: usize,
        distance: f32,
    },
}

#[cfg(all(feature = "mouse_input", any(feature = "2d", feature = "3d")))]
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            #[cfg(feature = "2d")]
            (SortKey::ZIndex(a), SortKey::ZIndex(b)) => b.partial_cmp(a),
            #[cfg(feature = "3d")]
            (
                SortKey::Distance {
                    camera_priority: ca,
                    distance: da,
                },
                SortKey::Distance {
                    camera_priority: cb,
                    distance: db,
                },
            ) => match ca.cmp(cb) {
                std::cmp::Ordering::Equal => da.partial_cmp(db),
                ord => Some(ord),
            },
            #[cfg(all(feature = "2d", feature = "3d"))]
            (SortKey::ZIndex(_), SortKey::Distance { .. }) => Some(std::cmp::Ordering::Less),
            #[cfg(all(feature = "2d", feature = "3d"))]
            (SortKey::Distance { .. }, SortKey::ZIndex(_)) => Some(std::cmp::Ordering::Greater),
        }
    }
}

/// A terminal under the cursor: the surface entity, the cell, and how it
/// ranks against other hits.
#[cfg(feature = "mouse_input")]
type Hit = (Entity, HitTestResult, SortKey);

/// Mouse, keyboard-modifier, touch and wheel input read by
/// [`mouse_input_system`].
#[cfg(feature = "mouse_input")]
#[derive(bevy::ecs::system::SystemParam)]
pub struct MouseInputs<'w, 's> {
    buttons: Res<'w, ButtonInput<MouseButton>>,
    keyboard: Res<'w, ButtonInput<BevyKeyCode>>,
    touches: Res<'w, Touches>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    cursor: Res<'w, CursorPosition>,
    config: Res<'w, TerminalInputConfig>,
}

/// Entities that make a mesh terminal rather than a UI one.
#[cfg(all(feature = "mouse_input", feature = "3d"))]
type HasMesh = Or<(With<Mesh3d>, With<Mesh2d>)>;

/// UI terminals: nodes, minus any that also carry a mesh (a mesh wins).
#[cfg(all(feature = "mouse_input", feature = "2d", feature = "3d"))]
type UiOnly = (With<bevy::ui::Node>, Without<Mesh3d>, Without<Mesh2d>);
#[cfg(all(feature = "mouse_input", feature = "2d", not(feature = "3d")))]
type UiOnly = With<bevy::ui::Node>;

/// 2D UI terminals and what can move them under a still cursor.
#[cfg(all(feature = "mouse_input", feature = "2d"))]
#[derive(bevy::ecs::system::SystemParam)]
pub struct UiTerminals<'w, 's> {
    terminals: Query<
        'w,
        's,
        (
            Entity,
            &'static TerminalInput,
            Option<&'static bevy::ui::Node>,
            Option<&'static bevy::ui::ComputedNode>,
            Option<&'static bevy::ui::UiGlobalTransform>,
            Option<&'static crate::bevy_plugin::TerminalDimensions>,
            Option<&'static bevy::ui::ZIndex>,
        ),
        UiOnly,
    >,
    changed: Query<
        'w,
        's,
        (),
        (
            With<TerminalInput>,
            Or<(
                Changed<bevy::ui::ComputedNode>,
                Changed<bevy::ui::UiGlobalTransform>,
            )>,
        ),
    >,
}

#[cfg(all(feature = "mouse_input", feature = "2d"))]
impl UiTerminals<'_, '_> {
    fn any_changed(&self) -> bool {
        !self.changed.is_empty()
    }

    /// Screen-space bounding-box hits, ranked by `ZIndex`.
    fn hit_test(&self, cursor_pos: Vec2, hits: &mut Vec<Hit>) {
        for (entity, input, node, computed, ui_transform, dimensions, z_index) in &self.terminals {
            if !input.mouse {
                continue;
            }
            if let Some(hit) =
                bounding_box_hit_test(cursor_pos, ui_transform, node, computed, dimensions)
            {
                let z = z_index.map(|z| z.0).unwrap_or(0);
                hits.push((entity, hit, SortKey::ZIndex(z)));
            }
        }
    }
}

/// 3D mesh terminals, the cameras that see them, and what can move them
/// under a still cursor.
#[cfg(all(feature = "mouse_input", feature = "3d"))]
#[derive(bevy::ecs::system::SystemParam)]
pub struct MeshTerminals<'w, 's> {
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform)>,
    meshes: Res<'w, Assets<bevy::mesh::Mesh>>,
    terminals: Query<
        'w,
        's,
        (
            Entity,
            &'static TerminalInput,
            Option<&'static GlobalTransform>,
            Option<&'static Mesh2d>,
            Option<&'static Mesh3d>,
            Option<&'static crate::bevy_plugin::TerminalDimensions>,
            Option<&'static bevy::camera::visibility::ViewVisibility>,
            Option<&'static bevy::camera::primitives::Aabb>,
        ),
        HasMesh,
    >,
    camera_changed: Query<
        'w,
        's,
        (),
        (
            With<Camera>,
            Or<(
                Changed<GlobalTransform>,
                Changed<Projection>,
                Changed<Camera>,
            )>,
        ),
    >,
    terminal_changed: Query<'w, 's, (), (With<TerminalInput>, Changed<GlobalTransform>)>,
}

#[cfg(all(feature = "mouse_input", feature = "3d"))]
impl MeshTerminals<'_, '_> {
    fn any_changed(&self) -> bool {
        !self.camera_changed.is_empty() || !self.terminal_changed.is_empty()
    }

    /// Ray hits from every active camera under the cursor (highest
    /// `Camera::order` first), ranked by camera, then distance.
    fn hit_test(&self, cursor_pos: Vec2, hits: &mut Vec<Hit>) {
        let mut cameras: Vec<(&Camera, &GlobalTransform)> = self
            .cameras
            .iter()
            .filter(|(camera, _)| camera.is_active)
            .collect();
        cameras.sort_by_key(|(camera, _)| std::cmp::Reverse(camera.order));
        let world_rays: Vec<bevy::prelude::Ray3d> = cameras
            .iter()
            .filter_map(|(camera, camera_transform)| {
                let viewport = camera.logical_viewport_rect()?;
                if !viewport.contains(cursor_pos) {
                    return None;
                }
                camera
                    .viewport_to_world(camera_transform, cursor_pos - viewport.min)
                    .ok()
            })
            .collect();

        for (entity, input, transform, mesh2d, mesh3d, dimensions, view_visibility, aabb) in
            &self.terminals
        {
            if !input.mouse {
                continue;
            }
            let mesh_handle = mesh3d.map(|m| &m.0).or_else(|| mesh2d.map(|m| &m.0));
            let Some(transform) = transform else {
                continue;
            };
            if view_visibility.is_some_and(|v| !v.get()) {
                continue;
            }
            for (camera_priority, ray) in world_rays.iter().enumerate() {
                if !ray_intersects_aabb(ray, transform, aabb) {
                    continue;
                }
                if let Some((hit, distance)) = mesh_handle.and_then(|handle| {
                    ray_cast_hit_test_inner(ray, transform, handle, &self.meshes, dimensions)
                }) {
                    hits.push((
                        entity,
                        hit,
                        SortKey::Distance {
                            camera_priority,
                            distance,
                        },
                    ));
                    break;
                }
            }
        }
    }
}

/// The grid cell at a position measured in cells (`col`, `row` may be
/// fractional or out of range), clamped into the grid.
#[cfg(all(feature = "mouse_input", any(feature = "2d", feature = "3d")))]
fn cell_at(col: f32, row: f32, cols: u16, rows: u16) -> (u16, u16) {
    let max_col = cols.saturating_sub(1) as f32;
    let max_row = rows.saturating_sub(1) as f32;
    (
        col.max(0.0).min(max_col) as u16,
        row.max(0.0).min(max_row) as u16,
    )
}

/// Perform 2D UI bounding box hit test.
#[cfg(all(feature = "mouse_input", feature = "2d"))]
fn bounding_box_hit_test(
    cursor_pos: bevy::math::Vec2,
    ui_transform: Option<&bevy::ui::UiGlobalTransform>,
    node: Option<&bevy::ui::Node>,
    computed: Option<&bevy::ui::ComputedNode>,
    dimensions: Option<&crate::bevy_plugin::TerminalDimensions>,
) -> Option<HitTestResult> {
    let (width_px, height_px, inverse_scale) = if let Some(comp) = computed {
        let node_size = comp.unrounded_size();
        (
            node_size.x * comp.inverse_scale_factor,
            node_size.y * comp.inverse_scale_factor,
            comp.inverse_scale_factor,
        )
    } else {
        let n = node?;
        match (n.width, n.height) {
            (bevy::ui::Val::Px(w), bevy::ui::Val::Px(h)) => (w, h, 1.0),
            _ => return None,
        }
    };

    // `UiGlobalTransform` gives the node's center in the cursor's top-left,
    // +Y-down space for any anchoring (plain `GlobalTransform` is absent on
    // UI nodes, and `Node.left/top` read 0 for opposite-edge anchors).
    let (node_min_x, node_min_y) = if let Some(t) = ui_transform {
        // Physical pixels, like `ComputedNode.size`: scale to logical pixels
        // or clicks miss by the window's scale factor (2x on Retina).
        let center = t.translation * inverse_scale;
        debug!(
            "UiGlobalTransform center=({:.1}, {:.1}), size=({:.1}x{:.1})",
            center.x, center.y, width_px, height_px
        );
        (center.x - width_px / 2.0, center.y - height_px / 2.0)
    } else if let Some(n) = node {
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

    let x_in = cursor_pos.x >= node_min_x && cursor_pos.x <= node_max_x;
    let y_in = cursor_pos.y >= node_min_y && cursor_pos.y <= node_max_y;

    if !x_in || !y_in {
        return None;
    }

    let (cols, rows, char_width, char_height) = if let Some(dims) = dimensions {
        (
            dims.cols as f32,
            dims.rows as f32,
            dims.char_width_px as f32,
            dims.char_height_px as f32,
        )
    } else {
        let cols = 80.0;
        let rows = 24.0;
        (cols, rows, width_px / cols, height_px / rows)
    };

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

    let (col, row) = cell_at(
        local_x / char_width,
        local_y / char_height,
        cols as u16,
        rows as u16,
    );

    debug!("Hit test result: col={}, row={}", col, row);

    Some(HitTestResult { col, row })
}

/// Coarse pre-check: does `world_ray` even reach the
/// mesh entity's bounding box? Rejects most terminals with a few float ops
/// before any mesh vertex data (positions/indices/UVs) is touched by the
/// precise test in `ray_cast_hit_test_inner`.
#[cfg(all(feature = "mouse_input", feature = "3d"))]
fn ray_intersects_aabb(
    world_ray: &bevy::prelude::Ray3d,
    mesh_transform: &bevy::transform::components::GlobalTransform,
    aabb: Option<&bevy::camera::primitives::Aabb>,
) -> bool {
    use bevy::math::{Dir3A, Vec3A};
    use bevy::shape::{Aabb3d, RayCast3d};

    let Some(aabb) = aabb else {
        return true;
    };

    let world_to_local = mesh_transform.affine().inverse();
    let local_origin = world_to_local.transform_point3(world_ray.origin);
    let local_direction = world_to_local.transform_vector3(*world_ray.direction);
    let Ok(local_direction) = Dir3A::try_from(Vec3A::from(local_direction)) else {
        return true;
    };

    let ray_cast = RayCast3d::new(local_origin, local_direction, f32::MAX);
    let aabb3d = Aabb3d::new(aabb.center, aabb.half_extents);
    ray_cast.aabb_intersection_at(&aabb3d).is_some()
}

/// Perform 3D ray-mesh hit test.
#[cfg(all(feature = "mouse_input", feature = "3d"))]
fn ray_cast_hit_test_inner(
    world_ray: &bevy::prelude::Ray3d,
    mesh_transform: &bevy::transform::components::GlobalTransform,
    mesh_handle: &bevy::asset::Handle<bevy::mesh::Mesh>,
    meshes: &bevy::asset::Assets<bevy::mesh::Mesh>,
    dimensions: Option<&crate::bevy_plugin::TerminalDimensions>,
) -> Option<(HitTestResult, f32)> {
    use bevy::mesh::VertexAttributeValues;
    use bevy::picking::mesh_picking::ray_cast::{Backfaces, ray_mesh_intersection};
    use bevy::prelude::*;

    let mesh = meshes.get(mesh_handle)?;

    let ray3d = *world_ray;
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

    let hit_with = |indices: Option<&[u32]>, wide: Option<&[u16]>| match wide {
        Some(indices) => ray_mesh_intersection(
            ray3d,
            &mesh_transform_affine,
            positions.as_slice(),
            vertex_normals,
            Some(indices),
            uvs,
            Backfaces::Cull,
        ),
        None => ray_mesh_intersection(
            ray3d,
            &mesh_transform_affine,
            positions.as_slice(),
            vertex_normals,
            indices,
            uvs,
            Backfaces::Cull,
        ),
    };
    let hit = match mesh.indices() {
        Some(bevy::mesh::Indices::U16(indices)) => hit_with(None, Some(indices.as_slice())),
        Some(bevy::mesh::Indices::U32(indices)) => hit_with(Some(indices.as_slice()), None),
        None => hit_with(None, None),
    }?;

    let uv = hit.uv?;

    let (cols, rows) = if let Some(dims) = dimensions {
        (dims.cols, dims.rows)
    } else {
        (80, 24)
    };

    let (col, row) = cell_at(uv.x * f32::from(cols), uv.y * f32::from(rows), cols, rows);

    debug!(
        "3D Hit Test: uv=({:.3},{:.3}) distance={:.1} cols={} rows={} -> grid=({},{})",
        uv.x, uv.y, hit.distance, cols, rows, col, row
    );

    Some((HitTestResult { col, row }, hit.distance))
}

/// Selects `Moved` vs `Drag(button)` from which mouse buttons (and touches,
/// pre-folded into `left` by the caller) are currently held.
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
/// `(x, y)` delta - sign only, no accumulation.
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

/// A mouse event of `kind` at a cell, for `target`.
#[cfg(feature = "mouse_input")]
fn mouse_event(
    target: Entity,
    kind: MouseEventKind,
    (column, row): (u16, u16),
    modifiers: KeyModifiers,
) -> TerminalEvent {
    TerminalEvent {
        target,
        input: InputEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }),
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
    let left = buttons.pressed(MouseButton::Left) || touches.first_pressed_position().is_some();
    let kind = move_kind(
        left,
        buttons.pressed(MouseButton::Right),
        buttons.pressed(MouseButton::Middle),
    );
    events.write(mouse_event(
        remap_to_tui(surface_entity, surfaces),
        kind,
        (col, row),
        modifiers,
    ));
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

            events.write(mouse_event(
                target,
                MouseEventKind::Down(button),
                (col, row),
                modifiers,
            ));
        }

        if buttons.just_released(button) || (touch && touches.any_just_released()) {
            events.write(mouse_event(
                target,
                MouseEventKind::Up(button),
                (col, row),
                modifiers,
            ));
        }
    }
}

/// Emits one `Scroll*` event per `MouseWheel` message at the given hit
/// cell.
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
        events.write(mouse_event(target, kind, (col, row), modifiers));
    }
}

/// What [`mouse_input_system`] remembers between frames.
#[cfg(feature = "mouse_input")]
#[derive(Default)]
pub struct MouseState {
    last_cursor_pos: Option<Vec2>,
    last_hovered: Option<(Entity, u16, u16)>,
}

/// Mouse input: hit-tests 2D UI terminals (screen-space boxes) and 3D mesh
/// terminals (camera rays), whichever features are on, and sends the
/// topmost hit its move, button, focus and scroll events.
#[cfg(feature = "mouse_input")]
pub fn mouse_input_system(
    mut inputs: MouseInputs,
    mut focus: ResMut<TerminalFocus>,
    #[cfg(feature = "2d")] ui: UiTerminals,
    #[cfg(feature = "3d")] meshes: MeshTerminals,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
    mut state: Local<MouseState>,
) {
    let MouseState {
        last_cursor_pos,
        last_hovered,
    } = &mut *state;
    let wheel_messages: Vec<MouseWheel> = inputs.wheel.read().copied().collect();
    let MouseInputs {
        buttons,
        keyboard,
        touches,
        cursor,
        config,
        ..
    } = inputs;

    let Some(cursor_pos) = cursor.position else {
        *last_cursor_pos = None;
        *last_hovered = None;
        return;
    };

    let cursor_moved = *last_cursor_pos != Some(cursor_pos);
    let button_transition = buttons.get_just_pressed().len() > 0
        || buttons.get_just_released().len() > 0
        || touches.any_just_pressed()
        || touches.any_just_released();
    #[cfg_attr(not(any(feature = "2d", feature = "3d")), allow(unused_mut))]
    let mut scene_changed = false;
    #[cfg(feature = "2d")]
    {
        scene_changed |= ui.any_changed();
    }
    #[cfg(feature = "3d")]
    {
        scene_changed |= meshes.any_changed();
    }
    if !cursor_moved && !button_transition && !scene_changed && wheel_messages.is_empty() {
        return;
    }
    *last_cursor_pos = Some(cursor_pos);
    let modifiers = read_modifiers(&keyboard);

    #[cfg_attr(not(any(feature = "2d", feature = "3d")), allow(unused_mut))]
    let mut hit_candidates: Vec<Hit> = Vec::new();
    #[cfg(feature = "3d")]
    meshes.hit_test(cursor_pos, &mut hit_candidates);
    #[cfg(feature = "2d")]
    ui.hit_test(cursor_pos, &mut hit_candidates);

    if hit_candidates.len() > 1 {
        debug!(
            "Multiple terminals hit at cursor ({:.1}, {:.1}): {:?}",
            cursor_pos.x, cursor_pos.y, hit_candidates
        );
    }
    #[cfg(any(feature = "2d", feature = "3d"))]
    hit_candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let Some(&(entity, hit_result, _)) = hit_candidates.first() else {
        *last_hovered = None;
        return;
    };
    let hovered = (entity, hit_result.col, hit_result.row);
    if *last_hovered != Some(hovered) {
        emit_mouse_move(
            entity,
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
        entity,
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
        remap_to_tui(entity, &surfaces),
        hit_result.col,
        hit_result.row,
        &wheel_messages,
        modifiers,
        &mut events,
    );
}

/// Window resize event system.
pub fn window_resize_system(
    mut resize_events: MessageReader<bevy::window::WindowResized>,
    terminals: Query<Entity, With<crate::setup::Tui>>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
) {
    for resize_event in resize_events.read() {
        let pixels = UVec2::new(resize_event.width as u32, resize_event.height as u32);

        for entity in terminals.iter() {
            events.write(TerminalEvent {
                target: remap_to_tui(entity, &surfaces),
                input: InputEvent::Resize { pixels },
            });
        }
    }
}

/// Terminal focus cycling system.
pub fn terminal_focus_system(
    keyboard: Res<ButtonInput<BevyKeyCode>>,
    mut focus: ResMut<TerminalFocus>,
    terminals: Query<(Entity, &TerminalInput)>,
    surfaces: Query<&crate::setup::TuiSurface>,
    mut events: MessageWriter<TerminalEvent>,
) {
    if !keyboard.just_pressed(BevyKeyCode::Tab) {
        return;
    }

    let mut terminal_entities: Vec<Entity> = terminals
        .iter()
        .filter(|(_, input)| input.keyboard)
        .map(|(entity, _)| entity)
        .collect();

    if terminal_entities.is_empty() {
        return;
    }

    terminal_entities.sort();

    let current_index = focus.focused.and_then(|focused| {
        terminal_entities
            .iter()
            .position(|&entity| entity == focused)
    });

    let next_index = match current_index {
        Some(idx) => (idx + 1) % terminal_entities.len(),
        None => 0,
    };

    let Some(&next_entity) = terminal_entities.get(next_index) else {
        return;
    };

    if focus.focused != Some(next_entity) {
        if let Some(old_focus) = focus.focused {
            events.write(TerminalEvent {
                target: remap_to_tui(old_focus, &surfaces),
                input: InputEvent::FocusLost,
            });
        }

        focus.focused = Some(next_entity);

        events.write(TerminalEvent {
            target: remap_to_tui(next_entity, &surfaces),
            input: InputEvent::FocusGained,
        });
    }
}

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
        use crate::input::MouseEventKind;
        use bevy::input::mouse::MouseButton;

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

    #[cfg(all(feature = "mouse_input", any(feature = "2d", feature = "3d")))]
    mod cell_at_tests {
        use super::super::cell_at;

        #[test]
        fn origin_maps_to_first_cell() {
            assert_eq!(cell_at(0.0, 0.0, 80, 24), (0, 0));
        }

        #[test]
        fn a_cell_boundary_belongs_to_the_next_cell() {
            assert_eq!(cell_at(1.0, 1.0, 80, 24), (1, 1));
            assert_eq!(cell_at(0.99, 0.99, 80, 24), (0, 0));
        }

        #[test]
        fn the_far_edge_clamps_to_the_last_cell_not_one_past_it() {
            assert_eq!(cell_at(80.0, 24.0, 80, 24), (79, 23));
            assert_eq!(cell_at(80.01, 24.01, 80, 24), (79, 23));
        }

        #[test]
        fn out_of_bounds_positions_clamp_instead_of_wrapping() {
            assert_eq!(cell_at(-6.25, -3.125, 80, 24), (0, 0));
            assert_eq!(cell_at(1250.0, 625.0, 80, 24), (79, 23));
            assert_eq!(cell_at(f32::NAN, f32::INFINITY, 80, 24), (0, 23));
        }

        #[test]
        fn single_cell_terminal_always_resolves_to_zero() {
            assert_eq!(cell_at(62.5, 31.25, 1, 1), (0, 0));
        }

        #[test]
        fn midpoint_maps_to_the_middle_of_the_grid() {
            assert_eq!(cell_at(40.0, 12.0, 80, 24), (40, 12));
        }
    }
}

#[cfg(test)]
mod cursor_position_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn world_with(windows: &[Option<Vec2>]) -> World {
        let mut world = World::new();
        world.init_resource::<CursorPosition>();
        world.init_resource::<Touches>();
        for &cursor in windows {
            let mut window = Window::default();
            window.set_cursor_position(cursor);
            world.spawn(window);
        }
        world
    }

    #[test]
    fn without_a_primary_window_the_window_holding_the_cursor_is_used() {
        let mut world = world_with(&[None, Some(Vec2::new(3.0, 4.0))]);
        world
            .run_system_once(update_cursor_position_system)
            .expect("system runs");
        assert_eq!(
            world.resource::<CursorPosition>().position,
            Some(Vec2::new(3.0, 4.0))
        );
    }

    #[test]
    fn with_no_window_the_position_is_cleared() {
        let mut world = world_with(&[]);
        world.resource_mut::<CursorPosition>().position = Some(Vec2::ONE);
        world
            .run_system_once(update_cursor_position_system)
            .expect("system runs");
        assert_eq!(world.resource::<CursorPosition>().position, None);
    }
}
