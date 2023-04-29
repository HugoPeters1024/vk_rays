use crate::rasterization_pipeline::{RasterizationPipeline, RasterizationPipelinePlugin};
use crate::raytracing_pipeline::{RaytracingPipeline, RaytracingPlugin};
use crate::render_image::Image;
use crate::vulkan_assets::{AddVulkanAsset, VulkanAssets, VkAssetCleanupPlaybook};
use crate::vulkan_cleanup::{VkCleanup, VulkanCleanupEvent, VulkanCleanupPlugin};
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

#[derive(Resource, Default)]
pub struct RenderResources {
    pub rt_pipeline: Handle<RaytracingPipeline>,
    pub quad_pipeline: Handle<RasterizationPipeline>,
    pub render_target: Handle<Image>,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        // Don't ask, shit will segfault otherwise
        let mut winit_settings = app.world.get_resource_mut::<WinitSettings>().unwrap();
        winit_settings.return_from_run = true;

        let mut system_state: SystemState<Query<&RawHandleWrapper, With<PrimaryWindow>>> =
            SystemState::new(&mut app.world);
        let query = system_state.get(&app.world);
        let whandles = query.get_single().unwrap();
        let render_device = RenderDevice::from_window(whandles);
        app.world.insert_resource(render_device);

        app.add_plugin(VulkanCleanupPlugin);

        let mut render_schedule = RenderSet::base_schedule();
        render_schedule.add_system(prepare_render_target.in_set(RenderSet::Prepare));
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
            .add_vulkan_asset::<crate::render_image::Image>();
    }
}

fn run_render_schedule(world: &mut World) {
    world.run_schedule(RenderSchedule);
}

fn prepare_render_target(
    swapchain: Query<&Swapchain>,
    mut render_resources: ResMut<RenderResources>,
    mut images: ResMut<Assets<Image>>,
) {
    let swapchain = swapchain.single();
    if images.get(&render_resources.render_target).is_none() {
        render_resources.render_target = images.add(Image {
            width: swapchain.width,
            height: swapchain.height,
            format: vk::Format::R32G32B32A32_SFLOAT,
            initial_layout: vk::ImageLayout::GENERAL,
            usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
        });
    }
}

fn wait_for_frame_finish(
    device: Res<RenderDevice>,
    cleanup: Res<VkCleanup>,
    mut swapchain: Query<&mut Swapchain>,
) {
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

    cleanup.send(VulkanCleanupEvent::SignalNextFrame);
}

fn render(
    device: Res<RenderDevice>,
    mut swapchain: Query<&mut Swapchain>,
    render_resources: Res<RenderResources>,
    rt_pipelines: Res<VulkanAssets<RaytracingPipeline>>,
    rast_pipelines: Res<VulkanAssets<RasterizationPipeline>>,
    vk_images: Res<VulkanAssets<Image>>,
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

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        device
            .device
            .begin_command_buffer(cmd_buffer, &begin_info)
            .unwrap();

        // Make swapchain available for rendering
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            swapchain_image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        );

        if let Some(render_target) = vk_images.get(&render_resources.render_target) {
            if let Some(compiled) = rt_pipelines.get(&render_resources.rt_pipeline) {
                // update the descriptor set
                let render_target_image_binding = vk::DescriptorImageInfo::builder()
                    .image_layout(vk::ImageLayout::GENERAL)
                    .image_view(render_target.view)
                    .build();

                let descriptor_write = vk::WriteDescriptorSet::builder()
                    .dst_set(compiled.descriptor_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .image_info(std::slice::from_ref(&render_target_image_binding))
                    .build();

                device
                    .device
                    .update_descriptor_sets(std::slice::from_ref(&descriptor_write), &[]);

                device.device.cmd_bind_pipeline(
                    cmd_buffer,
                    vk::PipelineBindPoint::RAY_TRACING_KHR,
                    compiled.vk_pipeline,
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
                render_target.handle,
                vk::ImageLayout::GENERAL,
                vk::ImageLayout::GENERAL,
            );

            if let Some(compiled) = rast_pipelines.get(&render_resources.quad_pipeline) {
                // update the descriptor set
                let render_target_image_binding = vk::DescriptorImageInfo::builder()
                    .image_layout(vk::ImageLayout::GENERAL)
                    .image_view(render_target.view)
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

                device.device.cmd_bind_pipeline(
                    cmd_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    compiled.vk_pipeline,
                );

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
            .wait_dst_stage_mask(std::slice::from_ref(
                &vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ))
            .signal_semaphores(std::slice::from_ref(&swapchain.render_finished_sem))
            .build();

        {
            let queue = device.queue.lock().unwrap();
            device
                .device
                .queue_submit(
                    queue.clone(),
                    std::slice::from_ref(&submit_info),
                    swapchain.fence,
                )
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
            device
                .exts
                .swapchain
                .queue_present(queue.clone(), &present_info)
        };

        match present_result {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::SUBOPTIMAL_KHR) => {
                let primary_window = primary_window.get_single().unwrap();
                swapchain.on_resize(primary_window);
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
