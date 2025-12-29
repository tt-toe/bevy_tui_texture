//! Ray casting utilities for 3D mouse input
//!
//! This module provides ray casting functionality for detecting mouse clicks
//! on 3D terminal meshes. It implements the Möller-Trumbore intersection algorithm
//! for ray-triangle intersection and handles UV coordinate interpolation.

use bevy::prelude::*;

/// A ray in 3D space with an origin point and direction vector.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    /// Create a new ray from origin and direction.
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self {
            origin,
            direction: direction.normalize(),
        }
    }

    /// Create a ray from camera through a screen position.
    ///
    /// # Arguments
    /// * `cursor_ndc` - Cursor position in Normalized Device Coordinates (-1 to 1)
    /// * `camera_transform` - Camera's global transform
    /// * `projection` - Camera's projection matrix
    /// * `viewport_size` - Viewport dimensions (width, height) in pixels
    ///
    /// # Returns
    /// Ray in world space shooting from camera through cursor position
    pub fn from_camera(
        cursor_ndc: Vec2,
        camera_transform: &GlobalTransform,
        projection: &Projection,
        viewport_size: Vec2,
    ) -> Self {
        match projection {
            Projection::Perspective(persp) => {
                Self::from_perspective_camera(cursor_ndc, camera_transform, persp)
            }
            Projection::Orthographic(ortho) => {
                Self::from_orthographic_camera(cursor_ndc, camera_transform, ortho, viewport_size)
            }
            Projection::Custom(_) => {
                // For custom projections, use a default forward ray
                let camera_pos = camera_transform.translation();
                let world_dir = *camera_transform.forward();
                Self::new(camera_pos, world_dir)
            }
        }
    }

    /// Create ray from perspective camera.
    fn from_perspective_camera(
        cursor_ndc: Vec2,
        camera_transform: &GlobalTransform,
        projection: &PerspectiveProjection,
    ) -> Self {
        // Get camera position (ray origin)
        let camera_pos = camera_transform.translation();

        // Build projection matrix
        let aspect = projection.aspect_ratio;
        let fov = projection.fov;

        // Calculate ray direction in view space
        let tan_half_fov = (fov / 2.0).tan();
        let view_x = cursor_ndc.x * tan_half_fov * aspect;
        let view_y = cursor_ndc.y * tan_half_fov;
        let view_dir = Vec3::new(view_x, view_y, -1.0).normalize();

        // Transform to world space using camera rotation
        let world_dir = camera_transform.rotation() * view_dir;

        Self::new(camera_pos, world_dir)
    }

    /// Create ray from orthographic camera.
    ///
    /// Coordinate System for Bevy Camera2d:
    /// - Camera looks down -Z axis (forward = Vec3::NEG_Z)
    /// - Up direction is +Y axis
    /// - Right direction is +X axis
    /// - Mesh plane is typically at Z=0 in the XY plane
    fn from_orthographic_camera(
        cursor_ndc: Vec2,
        camera_transform: &GlobalTransform,
        projection: &OrthographicProjection,
        viewport_size: Vec2,
    ) -> Self {
        // For orthographic projection, all rays are parallel and perpendicular to the view plane
        let scale = projection.scale;

        // Convert NDC to view space
        // NDC: (-1,-1) = bottom-left, (1,1) = top-right
        // View space: scaled by half-viewport-size * projection.scale
        let view_pos = Vec3::new(
            cursor_ndc.x * (viewport_size.x / 2.0) * scale,
            cursor_ndc.y * (viewport_size.y / 2.0) * scale,
            -projection.near, // Start at near plane
        );

        // Transform view space to world space using camera transform
        let world_pos = camera_transform.transform_point(view_pos);

        // All rays are parallel in orthographic projection, pointing in camera's forward direction
        // For Camera2d, forward is typically (0, 0, -1) in world space
        let world_dir = *camera_transform.forward(); // Deref Dir3 to Vec3

        Self::new(world_pos, world_dir)
    }

    /// Transform ray to local space of an entity.
    ///
    /// This is useful for performing intersection tests in the entity's local
    /// coordinate system.
    pub fn to_local(&self, transform: &GlobalTransform) -> Self {
        let inv_transform = transform.affine().inverse();

        Self {
            origin: inv_transform.transform_point3(self.origin),
            direction: inv_transform.transform_vector3(self.direction).normalize(),
        }
    }

    /// Get point at distance t along the ray.
    pub fn point_at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }
}

/// Result of a ray-mesh intersection test.
#[derive(Debug, Clone)]
pub struct RayHit {
    /// Point of intersection in local space
    pub point: Vec3,
    /// Surface normal at intersection point
    pub normal: Vec3,
    /// Distance from ray origin to hit point
    pub distance: f32,
    /// UV coordinates at hit point (if mesh has UV data)
    pub uv: Option<Vec2>,
    /// Barycentric coordinates of hit point (u, v, w)
    pub barycentric: Vec3,
}

/// Perform ray-triangle intersection using Möller-Trumbore algorithm.
///
/// # Arguments
/// * `ray` - Ray to test
/// * `v0`, `v1`, `v2` - Triangle vertices
///
/// # Returns
/// Some((distance, barycentric)) if hit, where:
/// - distance: t value along ray
/// - barycentric: (u, v, w) coordinates where w = 1 - u - v
///
/// Reference: https://en.wikipedia.org/wiki/Möller–Trumbore_intersection_algorithm
pub fn ray_triangle_intersection(ray: &Ray, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<(f32, Vec3)> {
    const EPSILON: f32 = 0.000001;

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;

    let h = ray.direction.cross(edge2);
    let a = edge1.dot(h);

    // Ray parallel to triangle
    if a.abs() < EPSILON {
        return None;
    }

    let f = 1.0 / a;
    let s = ray.origin - v0;
    let u = f * s.dot(h);

    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = s.cross(edge1);
    let v = f * ray.direction.dot(q);

    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * edge2.dot(q);

    // Ray intersection behind origin
    if t < EPSILON {
        return None;
    }

    let w = 1.0 - u - v;
    Some((t, Vec3::new(u, v, w)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_triangle_intersection_hit() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 2.0), Vec3::new(0.0, 0.0, -1.0));
        let v0 = Vec3::new(-1.0, -1.0, 0.0);
        let v1 = Vec3::new(1.0, -1.0, 0.0);
        let v2 = Vec3::new(0.0, 1.0, 0.0);

        let result = ray_triangle_intersection(&ray, v0, v1, v2);
        assert!(result.is_some());

        let (t, _bary) = result.unwrap();
        assert!((t - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_ray_triangle_intersection_miss() {
        let ray = Ray::new(Vec3::new(5.0, 5.0, 2.0), Vec3::new(0.0, 0.0, -1.0));
        let v0 = Vec3::new(-1.0, -1.0, 0.0);
        let v1 = Vec3::new(1.0, -1.0, 0.0);
        let v2 = Vec3::new(0.0, 1.0, 0.0);

        let result = ray_triangle_intersection(&ray, v0, v1, v2);
        assert!(result.is_none());
    }

    #[test]
    fn test_ray_triangle_barycentric() {
        let ray = Ray::new(Vec3::new(0.5, 0.0, 2.0), Vec3::new(0.0, 0.0, -1.0));
        let v0 = Vec3::new(0.0, 0.0, 0.0);
        let v1 = Vec3::new(1.0, 0.0, 0.0);
        let v2 = Vec3::new(0.0, 1.0, 0.0);

        let result = ray_triangle_intersection(&ray, v0, v1, v2);
        assert!(result.is_some());

        let (_t, bary) = result.unwrap();
        // Hit point (0.5, 0.0, 0.0) is near middle of v0-v1 edge
        // Barycentric coords: bary.x=u (v1), bary.y=v (v2), bary.z=w (v0)
        assert!(bary.x > 0.4 && bary.x < 0.6); // u value (v1) - should be ~0.5
        assert!(bary.y < 0.1); // v value (v2) - should be ~0.0
        assert!(bary.z > 0.4 && bary.z < 0.6); // w value (v0) - should be ~0.5
    }
}
