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
mod swapchain;
mod vk_utils;
mod vulkan_assets;
mod vulkan_cleanup;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use camera::{Camera3d, Camera3dBundle};
use clap::Parser;
use gltf_assets::GltfMesh;
use rand::RngCore;
use rasterization_pipeline::RasterizationPipeline;
use render_plugin::RenderConfig;
use shader::Shader;

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
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            watch_for_changes: true,
            ..default()
        }))
        .add_plugin(RenderPlugin)
        .add_startup_system(startup)
        .add_system(report_fps)
        .add_system(player_controls)
        .add_system(spawn)
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
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0.0, 0.0, -3.0),
        ..default()
    });

    let game_assets = GameAssets {
        box_mesh: assets.load("models/box.glb"),
    };

    commands.spawn((
        game_assets.box_mesh.clone(),
        TransformBundle::from_transform(Transform::from_rotation(
            Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.5),
        )),
        MainBlock,
    ));

    let raygen_shader: Handle<Shader> = assets.load("shaders/raygen.rgen");
    let hit_shader: Handle<Shader> = assets.load("shaders/hit.rchit");
    let miss_shader: Handle<Shader> = assets.load("shaders/miss.rmiss");

    let rt_pipeline = rt_pipelines.add(RaytracingPipeline {
        raygen_shader: raygen_shader.clone(),
        hit_shader: hit_shader.clone(),
        miss_shader: miss_shader.clone(),
        ..default()
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
    if rng.next_u32() % 100 == 0 {
        println!("FPS: {}", 1.0 / time.delta_seconds());
    }
}

fn player_controls(input: Res<Input<KeyCode>>, time: Res<Time>, mut camera: Query<&mut Transform, With<Camera3d>>) {
    let mut camera = camera.single_mut();
    let f = time.delta_seconds();

    let mut direction = Vec3::ZERO;
    if input.pressed(KeyCode::W) {
        direction += camera.local_z();
    }
    if input.pressed(KeyCode::S) {
        direction -= camera.local_z();
    }
    if input.pressed(KeyCode::A) {
        direction += camera.local_x();
    }
    if input.pressed(KeyCode::D) {
        direction -= camera.local_x();
    }
    if input.pressed(KeyCode::Q) {
        direction -= camera.local_y();
    }
    if input.pressed(KeyCode::E) {
        direction += camera.local_y();
    }

    if input.pressed(KeyCode::Left) {
        camera.rotation *= Quat::from_rotation_y(-f);
    }

    if input.pressed(KeyCode::Right) {
        camera.rotation *= Quat::from_rotation_y(f);
    }

    if direction.length_squared() > 0.0 {
        camera.translation += direction.normalize() * 3.0 * f;
    }
}

fn spawn(mut commands: Commands, game_assets: Res<GameAssets>, time: Res<Time>, mut done: Local<bool>) {
    if !*done && time.elapsed_seconds() > 0.0 {
        commands.spawn((
            game_assets.box_mesh.clone(),
            TransformBundle::from_transform(Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::new(10.0, 0.2, 10.0))),
        ));
        *done = true;
    }
}
