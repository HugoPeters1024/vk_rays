mod acceleration_structure;
mod camera;
mod composed_asset;
mod gltf_assets;
mod initializers;
mod rasterization_pipeline;
mod raytracing_pipeline;
mod render_buffer;
mod render_device;
mod render_image;
mod render_plugin;
mod scene;
mod shader;
mod shader_binding_table;
mod sphere_blas;
mod swapchain;
mod texture;
mod vk_utils;
mod vulkan_assets;
mod vulkan_cleanup;

use std::f32::consts::PI;
use std::time::Duration;

use bevy::asset::HandleId;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use bevy::window::PrimaryWindow;
use bevy_rapier3d::prelude::*;
use camera::{Camera3d, Camera3dBundle, PitchYaw};
use clap::Parser;
use gltf_assets::GltfMesh;
use rasterization_pipeline::RasterizationPipeline;
use render_plugin::{RayFocalFocus, RenderConfig};
use sphere_blas::Sphere;

use crate::raytracing_pipeline::RaytracingPipeline;
use crate::render_plugin::RenderPlugin;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value_t = false)]
    dump_schedule: bool,
}

#[derive(Resource, Default)]
struct GameAssets {
    box_mesh: Handle<GltfMesh>,
    sponza: Handle<GltfMesh>,
    bistro_interior: Handle<GltfMesh>,
    bistro_exterior: Handle<GltfMesh>,
    fireplace_room: Handle<GltfMesh>,
    rungholt: Handle<GltfMesh>,
}

#[derive(Component)]
struct MainBlock;

fn main() {
    App::new()
        .add_plugin(bevy::log::LogPlugin::default())
        .add_plugin(bevy::core::TaskPoolPlugin::default())
        .add_plugin(bevy::core::TypeRegistrationPlugin::default())
        .add_plugin(bevy::core::FrameCountPlugin::default())
        .add_plugin(bevy::time::TimePlugin::default())
        .add_plugin(bevy::transform::TransformPlugin::default())
        .add_plugin(bevy::hierarchy::HierarchyPlugin::default())
        .add_plugin(bevy::diagnostic::DiagnosticsPlugin::default())
        .add_plugin(bevy::input::InputPlugin::default())
        .add_plugin(bevy::window::WindowPlugin::default())
        .add_plugin(bevy::a11y::AccessibilityPlugin)
        .add_plugin(bevy::asset::AssetPlugin {
            watch_for_changes: true,
            ..default()
        })
        .add_plugin(bevy::asset::debug_asset_server::DebugAssetServerPlugin::default())
        .add_plugin(bevy::winit::WinitPlugin::default())
        .add_plugin(bevy::scene::ScenePlugin::default())
        .add_asset::<bevy::render::mesh::Mesh>()
        .add_asset_loader(bevy::render::texture::ExrTextureLoader)
        .add_plugin(RenderPlugin)
        .add_plugin(RapierPhysicsPlugin::<NoUserData>::default())
        .insert_resource(RapierConfiguration {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            ..default()
        })
        .add_startup_system(startup)
        .add_system(mouse_click)
        .add_system(move_sphere)
        .add_system(report_fps)
        .add_system(player_controls)
        .add_system(spawn.run_if(on_timer(Duration::from_secs_f32(0.02))))
        .run();

    std::thread::sleep(std::time::Duration::from_millis(100));
    println!("Goodbye!");
}

fn startup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rt_pipelines: ResMut<Assets<RaytracingPipeline>>,
    mut rast_pipelines: ResMut<Assets<RasterizationPipeline>>,
) {
    for i in 0..10 {
        commands.spawn((
            Sphere,
            TransformBundle::from_transform(
                Transform::from_translation(Vec3::new(-5.0 + i as f32, 0.5, -1.25 + (i as f32) / 4.0))
                    .with_scale(Vec3::splat(0.9)),
            ),
            RigidBody::Fixed,
            Collider::ball(0.5),
        ));
    }

    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0.0, 0.0, -3.0),
        ..default()
    });

    let game_assets = GameAssets {
        rungholt: assets.load("models/rungholt.glb"),
        ..default()
    };

    // floor
    //commands.spawn((
    //    game_assets.box_mesh.clone(),
    //    TransformBundle::from_transform(Transform::from_xyz(0.0, -1.0, 0.0).with_scale(Vec3::new(100.0, 0.2, 100.0))),
    //    RigidBody::Fixed,
    //    Collider::cuboid(0.5, 0.5, 0.5),
    //));
    //
    //commands.spawn((
    //    game_assets.bistro_interior.clone(),
    //    TransformBundle::from_transform(
    //        Transform::from_scale(Vec3::splat(0.01)).with_rotation(Quat::from_rotation_x(0.0)),
    //    ),
    //));
    //
    commands.spawn((
        game_assets.rungholt.clone(),
        TransformBundle::from_transform(
            Transform::from_scale(Vec3::splat(0.1)).with_rotation(Quat::from_rotation_x(PI/2.0)),
        ),
    ));

    commands.insert_resource(game_assets);

    commands.insert_resource(RenderConfig {
        rt_pipeline: rt_pipelines.add(RaytracingPipeline {
            raygen_shader: assets.load("shaders/raygen.rgen"),
            triangle_hit_shader: assets.load("shaders/hit.rchit"),
            miss_shader: assets.load("shaders/miss.rmiss"),
            sphere_int_shader: assets.load("shaders/sphere.rint"),
            sphere_hit_shader: assets.load("shaders/sphere.rchit"),
        }),
        quad_pipeline: rast_pipelines.add(RasterizationPipeline {
            vs_shader: assets.load("shaders/quad.vert"),
            fs_shader: assets.load("shaders/quad.frag"),
        }),
        skybox: assets.load("textures/sky.exr"),
    });
}

fn report_fps(time: Res<Time>, input: Res<Input<KeyCode>>, mut ravg: Local<f32>) {
    if input.just_pressed(KeyCode::Tab) {
        println!("Average FPS: {}", *ravg);
    }
    if *ravg == f32::INFINITY {
        *ravg = 1.0 / time.delta_seconds();
    }
    *ravg = *ravg * 0.95 + 0.05 * (1.0 / time.delta_seconds());
}

fn move_sphere(input: Res<Input<KeyCode>>, time: Res<Time>, mut spheres: Query<&mut Transform, With<Sphere>>) {
    let f = time.delta_seconds();
    for mut sphere in spheres.iter_mut() {
        if input.pressed(KeyCode::P) {
            sphere.translation += Vec3::Y * f;
        }

        if input.pressed(KeyCode::O) {
            sphere.translation -= Vec3::Y * f;
        }
    }
}

fn player_controls(
    input: Res<Input<KeyCode>>,
    time: Res<Time>,
    mut camera: Query<(&mut Transform, &mut PitchYaw), With<Camera3d>>,
) {
    let (mut camera, mut pitch_yaw) = camera.single_mut();
    let f = time.delta_seconds();

    // construct a vec3 that indicates the direction the player is looking
    let look_dir = (camera.rotation.inverse() * Vec3::new(0.0, 0.0, 1.0)).normalize();

    let speed = if input.pressed(KeyCode::LShift) { 4.0 } else { 1.0 };
    let sideways = Vec3::normalize(Vec3::cross(look_dir, Vec3::Y));

    if input.pressed(KeyCode::W) {
        camera.translation += look_dir * f * speed;
    }

    if input.pressed(KeyCode::S) {
        camera.translation -= look_dir * f * speed;
    }

    if input.pressed(KeyCode::A) {
        camera.translation -= sideways * f * speed;
    }

    if input.pressed(KeyCode::D) {
        camera.translation += sideways * f * speed;
    }

    if input.pressed(KeyCode::Q) {
        camera.translation -= Vec3::Y * f * speed;
    }

    if input.pressed(KeyCode::E) {
        camera.translation += Vec3::Y * f * speed;
    }

    if input.pressed(KeyCode::Left) {
        pitch_yaw.yaw -= f;
    }

    if input.pressed(KeyCode::Right) {
        pitch_yaw.yaw += f;
    }

    if input.pressed(KeyCode::Up) {
        pitch_yaw.pitch += f;
    }

    if input.pressed(KeyCode::Down) {
        pitch_yaw.pitch -= f;
    }

    camera.rotation = Quat::from_axis_angle(-Vec3::X, pitch_yaw.pitch) * Quat::from_axis_angle(Vec3::Y, pitch_yaw.yaw);
}

fn mouse_click(
    input: Res<Input<MouseButton>>,
    mut scroll_events: EventReader<MouseWheel>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut focus: ResMut<RayFocalFocus>,
    mut camera: Query<&mut Camera3d>,
) {
    if let Ok(mut camera) = camera.get_single_mut() {
        for scroll in scroll_events.iter() {
            camera.exposure += scroll.x * 0.1;
            camera.fov -= scroll.y * 0.1;
        }
    }
    if input.pressed(MouseButton::Left) {
        let window = window.single();
        if let Some(mouse_pos) = window.physical_cursor_position() {
            focus.0 = Some((mouse_pos.x as u32, mouse_pos.y as u32));
            return;
        }
    }

    focus.0 = None;
}

fn spawn(mut commands: Commands, game_assets: Res<GameAssets>, q: Query<&MainBlock>) {
    if q.iter().count() < 0 {
        commands.spawn((
            game_assets.box_mesh.clone(),
            TransformBundle::from_transform(
                Transform::default()
                    .with_rotation(Quat::from_rotation_y(PI / 2.0))
                    .with_scale(Vec3::splat(rand::random::<f32>()))
                    .with_translation(Vec3::new(
                        rand::random::<f32>() * 15.0 - 7.,
                        rand::random::<f32>() * 15.0 - 7.5,
                        rand::random::<f32>() * 15.0 - 7.5,
                    )),
            ),
            MainBlock,
            RigidBody::Dynamic,
            Collider::cuboid(0.5, 0.5, 0.5),
        ));
    }
}
