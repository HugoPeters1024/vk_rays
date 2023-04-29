mod composed_asset;
mod initializers;
mod rasterization_pipeline;
mod raytracing_pipeline;
mod render_buffer;
mod render_device;
mod render_image;
mod render_plugin;
mod shader;
mod swapchain;
mod vk_utils;
mod vulkan_assets;
mod vulkan_cleanup;

use bevy::app::AppExit;
use bevy::ecs::event::ManualEventReader;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use clap::Parser;
use rasterization_pipeline::RasterizationPipeline;
use render_plugin::RenderResources;
use shader::Shader;
use vulkan_assets::VkAssetCleanupPlaybook;
use vulkan_cleanup::VkCleanup;

use crate::raytracing_pipeline::RaytracingPipeline;
use crate::render_plugin::RenderPlugin;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value_t = false)]
    dump_schedule: bool,
}

fn main() {
    let args = Cli::parse();

    let mut app = App::new();
    if args.dump_schedule {
        app.add_plugins(DefaultPlugins.build().disable::<LogPlugin>());
    } else {
        app.add_plugins(DefaultPlugins.set(AssetPlugin {
            watch_for_changes: true,
            ..default()
        }));
    }

    app.add_plugin(RenderPlugin)
        .add_startup_system(startup)
        .add_system(shutdown.in_base_set(CoreSet::Last));

    if args.dump_schedule {
        bevy_mod_debugdump::print_main_schedule(&mut app);
    } else {
        app.run();
    }
}

fn startup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rt_pipelines: ResMut<Assets<RaytracingPipeline>>,
    mut rast_pipelines: ResMut<Assets<RasterizationPipeline>>,
) {
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

    commands.insert_resource(RenderResources {
        rt_pipeline,
        quad_pipeline,
        ..default()
    });
}

fn shutdown(world: &mut World) {
    let mut exit_reader = ManualEventReader::<AppExit>::default();
    let exit_events = world.get_resource::<Events<AppExit>>().unwrap();

    if exit_reader.iter(exit_events).last().is_some() {
        let mut cleanup_playbook = world.remove_resource::<VkAssetCleanupPlaybook>().unwrap();
        cleanup_playbook.run(world);

        let cleanup = world.remove_resource::<VkCleanup>().unwrap();
        cleanup.flush_and_die();
    }
}
