use std::f32::consts::PI;

use bevy::prelude::*;

#[derive(Component)]
pub struct Camera3d {
    pub fov: f32,
    pub min_t: f32,
    pub max_t: f32,
    pub clear: bool,
}

impl Default for Camera3d {
    fn default() -> Self {
        Self {
            fov: PI / 2.2,
            min_t: 0.0001,
            max_t: 1000.0,
            clear: true,
        }
    }
}

#[derive(Bundle, Default)]
pub struct Camera3dBundle {
    pub camera: Camera3d,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}