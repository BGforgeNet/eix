//! The view camera: `CCamera` and its static instance.
//!
//! Names follow ei-rebuild.

use core::mem::offset_of;

use super::Global;
use super::object::{TransformObject, Vector3};

/// The head of `CCamera`; never held by value.
///
/// `CCamera::Construct` 0x40C8B0 builds the transform object first (identity rotation, vtable 0x73BA38).
#[repr(C)]
#[derive(Debug)]
pub struct Camera {
    /// The camera's position and rotation.
    pub transform: TransformObject,
    _unknown54: [u8; 0xD0 - 0x54],
    /// The zoom rate the camera moves towards: `CCamera::SetZoomTarget` 0x40F420.
    pub zoom_target: f32,
    _unknown_d4: [u8; 0xE0 - 0xD4],
    /// The point looked at: `CCamera::SetFocus` 0x40F5C0.
    pub focus: Vector3,
    _unknown_ec: [u8; 0xF0 - 0xEC],
    /// Distance from the focus: `CCamera::SetDistancePitch` 0x411470.
    pub distance: f32,
    _unknown_f4: [u8; 0x104 - 0xF4],
    /// Pitch (0x411470).
    pub pitch: f32,
    _unknown108: [u8; 0x2E0 - 0x108],
    /// Near plane distance: `CCamera::GetRange` 0x40D380 returns the three.
    pub near: f32,
    /// Far plane distance (0x40D380).
    pub far: f32,
    /// Field of view (0x40D380).
    pub field_of_view: f32,
}

const _: () = {
    assert!(offset_of!(Camera, transform) == 0x0);
    assert!(offset_of!(Camera, zoom_target) == 0xD0);
    assert!(offset_of!(Camera, focus) == 0xE0);
    assert!(offset_of!(Camera, distance) == 0xF0);
    assert!(offset_of!(Camera, pitch) == 0x104);
    assert!(offset_of!(Camera, near) == 0x2E0);
    assert!(offset_of!(Camera, far) == 0x2E4);
    assert!(offset_of!(Camera, field_of_view) == 0x2E8);
};

/// The view camera, constructed in place by its static initialiser (0x40C885 calls `CCamera::Construct` 0x40C8B0).
pub const CAMERA: Global<Camera> = Global::at(eix_sites::DATA_79B2A8);

#[cfg(test)]
mod tests {
    use super::*;

    // Universal Mod's notes name 0x79B2C4 the camera position, 0x79B2DC its rotation and 0x79B388 its aim point.
    #[test]
    fn the_cameras_fields_are_the_known_globals() {
        let at = |offset: usize| CAMERA.address() + offset;
        assert_eq!(
            at(offset_of!(TransformObject, position)),
            eix_sites::DATA_79B2C4
        );
        assert_eq!(
            at(offset_of!(TransformObject, rotation)),
            eix_sites::DATA_79B2DC
        );
        assert_eq!(at(offset_of!(Camera, focus)), eix_sites::DATA_79B388);
    }
}
