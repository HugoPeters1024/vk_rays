mod acceleration_structure;
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

use std::time::Duration;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::time::common_conditions::on_fixed_timer;
use bevy::window::PresentMode;
use clap::Parser;
use gltf_assets::GltfMesh;
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
    let args = Cli::parse();

    let mut app = App::new();
    if args.dump_schedule {
        app.add_plugins(DefaultPlugins.build().disable::<LogPlugin>());
    } else {
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes: true,
                    ..default()
                }),
        );
    }

    app.add_plugin(RenderPlugin).add_startup_system(startup);

    app.add_system(report_fps.run_if(on_fixed_timer(Duration::from_secs(1))));
    app.add_system(step);
    app.add_system(spawn);

    if args.dump_schedule {
        bevy_mod_debugdump::print_main_schedule(&mut app);
    } else {
        app.run();
    }

    drop(app);
    std::thread::sleep(std::time::Duration::from_millis(300));
    println!("Goodbye!");
}

fn startup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rt_pipelines: ResMut<Assets<RaytracingPipeline>>,
    mut rast_pipelines: ResMut<Assets<RasterizationPipeline>>,
) {
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
    println!("FPS: {}", 1.0 / time.delta_seconds());
}

fn step(mut q: Query<&mut Transform, With<MainBlock>>) {
    for mut transform in q.iter_mut() {
        transform.rotate(Quat::from_rotation_y(0.01));
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
