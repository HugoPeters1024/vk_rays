use std::f32::consts::PI;

use bevy::prelude::*;

#[derive(Component, Debug, Clone)]
pub struct Camera3d {
    pub fov: f32,
    pub min_t: f32,
    pub max_t: f32,
    pub moved: bool,
    pub exposure: f32,
}

impl PartialEq for Camera3d {
    fn eq(&self, other: &Self) -> bool {
        self.fov == other.fov
            && self.min_t == other.min_t
            && self.max_t == other.max_t
    }
}

#[derive(Default, Component)]
pub struct PitchYaw {
    pub pitch: f32,
    pub yaw: f32,
}

impl Default for Camera3d {
    fn default() -> Self {
        Self {
            fov: PI / 3.0,
            min_t: 0.0001,
            max_t: 100.0,
            moved: false,
            exposure: 1.0,
        }
    }
}

#[derive(Bundle, Default)]
pub struct Camera3dBundle {
    pub camera: Camera3d,
    pub pitch_yaw: PitchYaw,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

pub struct Camera3dPlugin;

impl Plugin for Camera3dPlugin {
    fn build(&self, app: &mut App) {
        app.add_system(check_moved);
    }
}

fn check_moved(
    mut query: Query<(&GlobalTransform, &mut Camera3d)>,
    mut last: Local<Option<(GlobalTransform, Camera3d)>>,
) {
    if let Some((last_transform, last_camera)) = last.as_mut() {
        for (transform, mut camera) in query.iter_mut() {
            if transform != last_transform || *camera != *last_camera {
                camera.moved = true;
                *last_transform = *transform;
                *last_camera = camera.clone();
            } else {
                camera.moved = false;
            }
        }
    } else {
        *last = Some((*query.single().0, query.single().1.clone()));
    }
}
