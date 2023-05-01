use std::f32::consts::PI;

use crate::rasterization_pipeline::{RasterizationPipeline, RasterizationPipelinePlugin};
use crate::raytracing_pipeline::{RaytracerRegisters, RaytracingPipeline, RaytracingPlugin};
use crate::render_buffer::{Buffer, BufferProvider};
use crate::render_image::{vk_image_from_asset, Image, VkImage};
use crate::scene::{Scene, ScenePlugin};
use crate::vulkan_assets::{AddVulkanAsset, VkAssetCleanupPlaybook, VulkanAssets};
use crate::vulkan_cleanup::{VkCleanup, VkCleanupEvent, VkCleanupPlugin};
use crate::{render_device::RenderDevice, swapchain::Swapchain};
use crate::{swapchain, vk_utils};
use ash::vk;
use bevy::app::AppExit;
use bevy::ecs::event::ManualEventReader;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::winit::WinitSettings;
use bevy::{
    ecs::system::SystemState,
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper},
};

#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct RenderSchedule;

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum RenderSet {
    Prepare,
    Extract,
    Render,
}

impl RenderSet {
    pub fn base_schedule() -> Schedule {
        use RenderSet::*;

        let mut schedule = Schedule::new();
        schedule.add_systems((
            flush_ecs.in_set(Prepare),
            flush_ecs.in_set(Extract),
            flush_ecs.in_set(Render),
        ));

        schedule.configure_sets((Prepare, Extract, Render).chain());
        schedule.set_executor_kind(bevy::ecs::schedule::ExecutorKind::SingleThreaded);

        schedule
    }
}

#[allow(unused_variables)]
fn flush_ecs(world: &mut World) {}

#[derive(Resource)]
pub struct RenderConfig {
    pub rt_pipeline: Handle<RaytracingPipeline>,
    pub quad_pipeline: Handle<RasterizationPipeline>,
}

#[derive(Resource)]
pub struct RenderResources {
    pub render_target: VkImage,
    pub uniform_buffer: Buffer<UniformData>,
}

fn cleanup_render_resources(render_resources: Res<RenderResources>, cleanup: Res<VkCleanup>) {
    cleanup.send(VkCleanupEvent::ImageView(render_resources.render_target.view));
    cleanup.send(VkCleanupEvent::Image(render_resources.render_target.handle));
    cleanup.send(VkCleanupEvent::Buffer(render_resources.uniform_buffer.handle));
}

pub struct UniformData {
    inverse_view: Mat4,
    inverse_proj: Mat4,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        // Don't ask, shit will segfault otherwise
        let mut winit_settings = app.world.get_resource_mut::<WinitSettings>().unwrap();
        winit_settings.return_from_run = true;

        let mut system_state: SystemState<Query<(&Window, &RawHandleWrapper), With<PrimaryWindow>>> =
            SystemState::new(&mut app.world);
        let query = system_state.get(&app.world);
        let (window, whandles) = query.get_single().unwrap();
        let render_device = RenderDevice::from_window(whandles);
        let window_width = window.physical_width();
        let window_height = window.physical_height();
        app.world.insert_resource(render_device.clone());

        app.add_plugin(VkCleanupPlugin);

        let mut render_schedule = RenderSet::base_schedule();
        render_schedule.add_system(wait_for_frame_finish.in_set(RenderSet::Prepare));
        render_schedule.add_system(render.in_set(RenderSet::Render));

        app.add_schedule(RenderSchedule, render_schedule);

        app.add_plugin(swapchain::SwapchainPlugin);
        app.add_plugin(RaytracingPlugin);
        app.add_plugin(RasterizationPipelinePlugin);

        app.add_system(run_render_schedule);
        app.add_system(shutdown.in_base_set(CoreSet::Last));

        app.add_asset::<crate::shader::Shader>()
            .init_asset_loader::<crate::shader::ShaderLoader>()
            .init_debug_asset_loader::<crate::shader::ShaderLoader>()
            .add_asset::<crate::render_image::Image>()
            .add_vulkan_asset::<crate::render_image::Image>()
            .add_asset::<crate::gltf_assets::GltfMesh>()
            .add_vulkan_asset::<crate::gltf_assets::GltfMesh>()
            .init_asset_loader::<crate::gltf_assets::GltfLoader>()
            .init_debug_asset_loader::<crate::gltf_assets::GltfLoader>();

        app.add_plugin(ScenePlugin);

        app.world
            .get_resource_mut::<VkAssetCleanupPlaybook>()
            .unwrap()
            .add_system(cleanup_render_resources);

        let swapchain = app.world.query::<&Swapchain>().single(&mut app.world);

        let render_target = vk_image_from_asset(
            &render_device,
            &Image {
                width: swapchain.width,
                height: swapchain.height,
                format: vk::Format::R32G32B32A32_SFLOAT,
                usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
                initial_layout: vk::ImageLayout::UNDEFINED,
            },
        );

        unsafe {
            render_device.run_single_commands(&|cmd_buffer| {
                vk_utils::transition_image_layout(
                    &render_device,
                    cmd_buffer,
                    render_target.handle,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::GENERAL,
                );
            });
        }

        app.world.insert_resource(RenderResources {
            render_target,
            uniform_buffer: render_device.create_host_buffer::<UniformData>(1, vk::BufferUsageFlags::UNIFORM_BUFFER),
        });
    }
}

fn run_render_schedule(world: &mut World) {
    world.run_schedule(RenderSchedule);
}

fn wait_for_frame_finish(device: Res<RenderDevice>, cleanup: Res<VkCleanup>, mut swapchain: Query<&mut Swapchain>) {
    let mut swapchain = swapchain.single_mut();
    unsafe {
        device
            .device
            .wait_for_fences(std::slice::from_ref(&swapchain.fence), true, u64::MAX)
            .unwrap();
        device
            .device
            .reset_fences(std::slice::from_ref(&swapchain.fence))
            .unwrap();
    }
    // get the next image to render to
    swapchain.aquire_next_image(&device);

    cleanup.send(VkCleanupEvent::SignalNextFrame);
}

fn render(
    device: Res<RenderDevice>,
    cleanup: Res<VkCleanup>,
    scene: Res<Scene>,
    time: Res<Time>,
    mut swapchain: Query<&mut Swapchain>,
    render_config: Res<RenderConfig>,
    mut render_resources: ResMut<RenderResources>,
    rt_pipelines: Res<VulkanAssets<RaytracingPipeline>>,
    rast_pipelines: Res<VulkanAssets<RasterizationPipeline>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let mut swapchain = swapchain.single_mut();

    // wait for the previous frame to finish
    unsafe {
        let (swapchain_image, swapchain_view) = swapchain.current_framebuffer();

        let cmd_buffer = device.cmd_buffer;
        device
            .device
            .reset_command_buffer(cmd_buffer, vk::CommandBufferResetFlags::empty())
            .unwrap();

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        device.device.begin_command_buffer(cmd_buffer, &begin_info).unwrap();

        // Make swapchain available for rendering
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            swapchain_image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        );

        if let Some(compiled) = rt_pipelines.get(&render_config.rt_pipeline) {
            if let Scene::Ready(tlas) = scene.into_inner() {
                // update the descriptor set
                let render_target_image_binding = vk::DescriptorImageInfo::builder()
                    .image_layout(vk::ImageLayout::GENERAL)
                    .image_view(render_resources.render_target.view)
                    .build();

                let write_render_target = vk::WriteDescriptorSet::builder()
                    .dst_set(compiled.descriptor_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .image_info(std::slice::from_ref(&render_target_image_binding))
                    .build();

                let mut p_acceleration_structure_write = vk::WriteDescriptorSetAccelerationStructureKHR::builder()
                    .acceleration_structures(std::slice::from_ref(&tlas.handle))
                    .build();

                let mut write_acceleration_structure = vk::WriteDescriptorSet::builder()
                    .dst_set(compiled.descriptor_set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                    .push_next(&mut p_acceleration_structure_write)
                    .build();
                write_acceleration_structure.descriptor_count = 1;

                device
                    .device
                    .update_descriptor_sets(&[write_render_target, write_acceleration_structure], &[]);

                device.device.cmd_bind_pipeline(
                    cmd_buffer,
                    vk::PipelineBindPoint::RAY_TRACING_KHR,
                    compiled.vk_pipeline,
                );

                {
                    let mut uniform_view = device.map_buffer(&mut render_resources.uniform_buffer);
                    let translation = Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0));
                    let rotation = Mat4::from_rotation_y(time.elapsed_seconds());
                    uniform_view[0].inverse_view = (translation * rotation).inverse();
                    uniform_view[0].inverse_proj =
                        Mat4::perspective_rh(PI / 2.0, swapchain.width as f32 / swapchain.height as f32, 0.001, 100.0)
                            .inverse();
                }

                let push_constants = RaytracerRegisters {
                    uniform_buffer_address: render_resources.uniform_buffer.address,
                };
                device.device.cmd_push_constants(
                    cmd_buffer,
                    compiled.pipeline_layout,
                    vk::ShaderStageFlags::RAYGEN_KHR,
                    0,
                    bytemuck::bytes_of(&push_constants),
                );

                device.device.cmd_bind_descriptor_sets(
                    cmd_buffer,
                    vk::PipelineBindPoint::RAY_TRACING_KHR,
                    compiled.pipeline_layout,
                    0,
                    std::slice::from_ref(&compiled.descriptor_set),
                    &[],
                );

                device.exts.rt_pipeline.cmd_trace_rays(
                    cmd_buffer,
                    &compiled.shader_binding_table.get_sbt_raygen(),
                    &compiled.shader_binding_table.get_sbt_miss(),
                    &compiled.shader_binding_table.get_sbt_hit(),
                    &vk::StridedDeviceAddressRegionKHR::default(),
                    swapchain.width,
                    swapchain.height,
                    1,
                )
            }

            // make render target available for sampling
            vk_utils::transition_image_layout(
                &device,
                cmd_buffer,
                render_resources.render_target.handle,
                vk::ImageLayout::GENERAL,
                vk::ImageLayout::GENERAL,
            );

            if let Some(compiled) = rast_pipelines.get(&render_config.quad_pipeline) {
                // update the descriptor set
                let render_target_image_binding = vk::DescriptorImageInfo::builder()
                    .image_layout(vk::ImageLayout::GENERAL)
                    .image_view(render_resources.render_target.view)
                    .sampler(device.nearest_sampler)
                    .build();

                let descriptor_write = vk::WriteDescriptorSet::builder()
                    .dst_set(compiled.descriptor_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(std::slice::from_ref(&render_target_image_binding))
                    .build();

                device
                    .device
                    .update_descriptor_sets(std::slice::from_ref(&descriptor_write), &[]);

                let render_area = vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D {
                        width: swapchain.width,
                        height: swapchain.height,
                    },
                };

                let attachment_info = vk::RenderingAttachmentInfoKHR::builder()
                    .image_view(swapchain_view)
                    .image_layout(vk::ImageLayout::ATTACHMENT_OPTIMAL)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .clear_value(vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [0.0, 0.0, 0.0, 0.0],
                        },
                    });

                let render_info = vk::RenderingInfo::builder()
                    .layer_count(1)
                    .render_area(render_area)
                    .color_attachments(std::slice::from_ref(&attachment_info));

                device.device.cmd_begin_rendering(cmd_buffer, &render_info);

                device
                    .device
                    .cmd_set_scissor(cmd_buffer, 0, std::slice::from_ref(&render_area));
                device.device.cmd_set_viewport(
                    cmd_buffer,
                    0,
                    std::slice::from_ref(&vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: swapchain.width as f32,
                        height: swapchain.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }),
                );

                device
                    .device
                    .cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::GRAPHICS, compiled.vk_pipeline);

                device.device.cmd_bind_descriptor_sets(
                    cmd_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    compiled.pipeline_layout,
                    0,
                    std::slice::from_ref(&compiled.descriptor_set),
                    &[],
                );

                device.device.cmd_draw(cmd_buffer, 3, 1, 0, 0);
                device.device.cmd_end_rendering(cmd_buffer);
            }
        }

        // Make swapchain available for presentation
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            swapchain_image,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::ImageLayout::PRESENT_SRC_KHR,
        );

        device.device.end_command_buffer(cmd_buffer).unwrap();

        // submit the command buffer to the queue
        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(std::slice::from_ref(&cmd_buffer))
            .wait_semaphores(std::slice::from_ref(&swapchain.image_ready_sem))
            .wait_dst_stage_mask(std::slice::from_ref(&vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT))
            .signal_semaphores(std::slice::from_ref(&swapchain.render_finished_sem))
            .build();

        {
            let queue = device.queue.lock().unwrap();
            device
                .device
                .queue_submit(queue.clone(), std::slice::from_ref(&submit_info), swapchain.fence)
                .unwrap();
        }

        let image_idx = swapchain.current_image_idx as u32;

        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(std::slice::from_ref(&swapchain.render_finished_sem))
            .swapchains(std::slice::from_ref(&swapchain.handle))
            .image_indices(std::slice::from_ref(&image_idx))
            .build();

        let present_result = {
            let queue = device.queue.lock().unwrap();
            device.exts.swapchain.queue_present(queue.clone(), &present_info)
        };

        match present_result {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::SUBOPTIMAL_KHR) => {
                println!("------ SWAPCHAIN OUT OF DATE ------");
                let primary_window = primary_window.get_single().unwrap();
                swapchain.on_resize(primary_window);

                let render_target = vk_image_from_asset(
                    &device,
                    &Image {
                        width: swapchain.width,
                        height: swapchain.height,
                        format: vk::Format::R32G32B32A32_SFLOAT,
                        usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                    },
                );

                device.run_single_commands(&|cmd_buffer| {
                    vk_utils::transition_image_layout(
                        &device,
                        cmd_buffer,
                        render_target.handle,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::GENERAL,
                    );
                });

                cleanup.send(VkCleanupEvent::Image(render_resources.render_target.handle));
                render_resources.render_target = render_target;
            }
            Err(e) => panic!("Failed to present swapchain image: {:?}", e),
            Ok(_) => {}
        }
    }
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
