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
mod sphere_blas;
mod swapchain;
mod vk_utils;
mod vulkan_assets;
mod vulkan_cleanup;

use std::f32::consts::PI;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use bevy_rapier3d::prelude::*;
use camera::{Camera3d, Camera3dBundle};
use clap::Parser;
use gltf_assets::GltfMesh;
use rand::RngCore;
use rasterization_pipeline::RasterizationPipeline;
use render_plugin::RenderConfig;
use shader::Shader;
use sphere_blas::{Sphere, SphereBLAS};

use crate::raytracing_pipeline::RaytracingPipeline;
use crate::render_plugin::RenderPlugin;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value_t = false)]
    dump_schedule: bool,
}

#[derive(Resource)]
struct GameAssets {
    box_mesh: Handle<GltfMesh>,
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
        .add_plugin(RenderPlugin)
        .add_plugin(RapierPhysicsPlugin::<NoUserData>::default())
        .insert_resource(RapierConfiguration {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            ..default()
        })
        .add_startup_system(startup)
        .add_system(camera_clear)
        .add_system(report_fps)
        .add_system(player_controls)
        .add_system(spawn.run_if(on_timer(Duration::from_secs_f32(0.02))))
        .run();

    std::thread::sleep(std::time::Duration::from_millis(300));
    println!("Goodbye!");
}

fn startup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rt_pipelines: ResMut<Assets<RaytracingPipeline>>,
    mut rast_pipelines: ResMut<Assets<RasterizationPipeline>>,
) {
    commands.spawn((
        Sphere,
        TransformBundle::from_transform(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)).with_scale(Vec3::splat(2.0))),
        RigidBody::Fixed,
        Collider::ball(0.5),
    ));
    commands.spawn((
        Sphere,
        TransformBundle::from_transform(
            Transform::from_translation(Vec3::new(2.5, 1.5, 0.0)).with_scale(Vec3::splat(2.0)),
        ),
        RigidBody::Fixed,
        Collider::ball(0.5),
    ));
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0.0, 0.0, -3.0),
        ..default()
    });

    let game_assets = GameAssets {
        box_mesh: assets.load("models/box.glb"),
    };

    // floor
    commands.spawn((
        game_assets.box_mesh.clone(),
        TransformBundle::from_transform(Transform::from_xyz(0.0, -1.0, 0.0).with_scale(Vec3::new(100.0, 0.2, 100.0))),
        RigidBody::Fixed,
        Collider::cuboid(0.5, 0.5, 0.5),
    ));

    let raygen_shader: Handle<Shader> = assets.load("shaders/raygen.rgen");
    let triangle_hit_shader: Handle<Shader> = assets.load("shaders/hit.rchit");
    let miss_shader: Handle<Shader> = assets.load("shaders/miss.rmiss");
    let sphere_int_shader: Handle<Shader> = assets.load("shaders/sphere.rint");
    let sphere_hit_shader: Handle<Shader> = assets.load("shaders/sphere.rchit");

    let rt_pipeline = rt_pipelines.add(RaytracingPipeline {
        raygen_shader: raygen_shader.clone(),
        triangle_hit_shader: triangle_hit_shader.clone(),
        miss_shader: miss_shader.clone(),
        sphere_int_shader,
        sphere_hit_shader,
    });

    let vs_shader: Handle<Shader> = assets.load("shaders/quad.vert");
    let fs_shader: Handle<Shader> = assets.load("shaders/quad.frag");

    let quad_pipeline = rast_pipelines.add(RasterizationPipeline {
        vs_shader,
        fs_shader,
        ..default()
    });

    commands.insert_resource(game_assets);

    commands.insert_resource(RenderConfig {
        rt_pipeline,
        quad_pipeline,
    });
}

fn report_fps(time: Res<Time>) {
    let mut rng = rand::thread_rng();
    if rng.next_u32() % 1000 == 0 {
        println!("FPS: {}", 1.0 / time.delta_seconds());
    }
}

fn player_controls(input: Res<Input<KeyCode>>, time: Res<Time>, mut camera: Query<&mut Transform, With<Camera3d>>) {
    let mut camera = camera.single_mut();
    let f = time.delta_seconds();

    // construct a vec3 that indicates the direction the player is looking
    let look_dir = camera.rotation.inverse() * Vec3::new(0.0, 0.0, 1.0);

    let sideways = Vec3::cross(look_dir, Vec3::Y);

    if input.pressed(KeyCode::W) {
        camera.translation += look_dir * f;
    }

    if input.pressed(KeyCode::S) {
        camera.translation -= look_dir * f;
    }

    if input.pressed(KeyCode::A) {
        camera.translation -= sideways * f;
    }

    if input.pressed(KeyCode::D) {
        camera.translation += sideways * f;
    }

    if input.pressed(KeyCode::Q) {
        camera.translation -= Vec3::Y * f;
    }

    if input.pressed(KeyCode::E) {
        camera.translation += Vec3::Y * f;
    }

    if input.pressed(KeyCode::Left) {
        camera.rotation *= Quat::from_rotation_y(-f);
    }

    if input.pressed(KeyCode::Right) {
        camera.rotation *= Quat::from_rotation_y(f);
    }
}

fn camera_clear(input: Res<Input<KeyCode>>, mut q: Query<&mut Camera3d>) {
    let mut camera = q.single_mut();
    if input.just_pressed(KeyCode::Space) {
        camera.clear = !camera.clear;
    }
}

fn spawn(mut commands: Commands, game_assets: Res<GameAssets>, q: Query<&MainBlock>) {
    if q.iter().count() < 300 {
        commands.spawn((
            game_assets.box_mesh.clone(),
            TransformBundle::from_transform(
                Transform::default()
                    .with_rotation(Quat::from_rotation_y(PI / 2.0))
                    .with_translation(Vec3::new(
                        rand::random::<f32>() * 10.0 - 5.0,
                        5.0,
                        rand::random::<f32>() * 10.0 - 5.0,
                    )),
            ),
            MainBlock,
            RigidBody::Dynamic,
            Collider::cuboid(0.5, 0.5, 0.5),
        ));
    }
}
