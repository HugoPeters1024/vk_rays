mod initializers;
mod vk_utils;
mod raytracing_pipeline;
mod render_buffer;
mod render_image;
mod render_device;
mod render_plugin;
mod shader;
mod swapchain;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use clap::Parser;
use raytracing_pipeline::RaytracingPipelineRef;
use shader::Shader;

use crate::raytracing_pipeline::RaytracingPipeline;
use crate::render_device::RenderDevice;
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
        .add_system(test);


    if args.dump_schedule {
        bevy_mod_debugdump::print_main_schedule(&mut app);
    } else {
        app.run();
    }
}

fn startup(mut commands: Commands, assets: Res<AssetServer>, mut pipelines: ResMut<Assets<RaytracingPipeline>>) {
    let raygen_shader: Handle<Shader> = assets.load("shaders/raygen.rgen");
    let hit_shader: Handle<Shader> = assets.load("shaders/hit.rchit");
    let miss_shader: Handle<Shader> = assets.load("shaders/miss.rmiss");
    commands.spawn(RaytracingPipelineRef {
        pipeline: pipelines.add(RaytracingPipeline {
            raygen_shader: raygen_shader.clone(),
            hit_shader: hit_shader.clone(),
            miss_shader: miss_shader.clone(),
        }),
    });
}

fn test(keyboard: Res<Input<KeyCode>>, time: Res<Time>, device: Res<RenderDevice>) {
    if keyboard.just_pressed(KeyCode::F) {
        println!(
            "[{}] fps: {}",
            device.device_name(),
            1.0 / time.delta_seconds()
        );
    }
}
