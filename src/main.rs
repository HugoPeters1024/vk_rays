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

use ash::vk;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use clap::Parser;
use gltf_assets::GltfMesh;
use rasterization_pipeline::RasterizationPipeline;
use render_buffer::BufferProvider;
use render_device::RenderDevice;
use render_plugin::{RenderResources, UniformData};
use shader::Shader;
use vulkan_cleanup::{VkCleanup, VkCleanupEvent};

use crate::raytracing_pipeline::RaytracingPipeline;
use crate::render_plugin::RenderPlugin;
use crate::vulkan_assets::VkAssetCleanupPlaybook;

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

    app.add_plugin(RenderPlugin).add_startup_system(startup);
    app.world
        .get_resource_mut::<VkAssetCleanupPlaybook>()
        .unwrap()
        .add_system(cleanup_render_resources);

    if args.dump_schedule {
        bevy_mod_debugdump::print_main_schedule(&mut app);
    } else {
        app.run();
    }

    println!("Goodbye!");
}

#[derive(Resource)]
struct Lol {
    box_mesh: Handle<GltfMesh>,
}

fn startup(
    mut commands: Commands,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    mut rt_pipelines: ResMut<Assets<RaytracingPipeline>>,
    mut rast_pipelines: ResMut<Assets<RasterizationPipeline>>,
) {
    let box_mesh: Handle<GltfMesh> = assets.load("models/box.glb");
    commands.spawn(box_mesh);

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
        render_target: Handle::default(),
        uniform_buffer: device
            .create_host_buffer::<UniformData>(1, vk::BufferUsageFlags::UNIFORM_BUFFER),
    });
}

fn cleanup_render_resources(cleanup: Res<VkCleanup>, resources: Res<RenderResources>) {
    cleanup.send(VkCleanupEvent::Buffer(resources.uniform_buffer.handle));
}
