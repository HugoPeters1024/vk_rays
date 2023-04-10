use crate::rasterization_pipeline::{RasterizationPipeline, RasterizationPipelinePlugin};
use crate::raytracing_pipeline::{RaytracingPipeline, RaytracingPlugin};
use crate::render_image::{Image, ImageProvider};
use crate::vulkan_asset_server::VulkanAssets;
use crate::{swapchain, vk_utils};
use crate::{render_device::RenderDevice, swapchain::Swapchain};
use ash::vk;
use bevy::ecs::schedule::ScheduleLabel;
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
        app.add_asset::<crate::shader::Shader>()
            .init_asset_loader::<crate::shader::ShaderLoader>()
            .init_debug_asset_loader::<crate::shader::ShaderLoader>()
            .add_asset::<crate::render_image::Image>();

        let mut system_state: SystemState<Query<&RawHandleWrapper, With<PrimaryWindow>>> =
            SystemState::new(&mut app.world);
        let query = system_state.get(&app.world);

        let whandles = query.get_single().unwrap();
        let render_device = RenderDevice::from_window(whandles);
        app.world.insert_resource(render_device);

        let mut render_schedule = RenderSet::base_schedule();
        render_schedule.add_system(prepare_render_target.in_set(RenderSet::Prepare));
        render_schedule.add_system(wait_for_frame_finish.in_set(RenderSet::Prepare));
        render_schedule.add_system(render.in_set(RenderSet::Render));

        app.add_schedule(RenderSchedule, render_schedule);

        app.add_plugin(swapchain::SwapchainPlugin);
        app.add_plugin(RaytracingPlugin);
        app.add_plugin(RasterizationPipelinePlugin);

        app.add_system(run_render_schedule);
    }
}

fn run_render_schedule(world: &mut World) {
    world.run_schedule(RenderSchedule);
}

fn prepare_render_target(
    device: Res<RenderDevice>,
    swapchain: Res<Swapchain>,
    mut render_resources: ResMut<RenderResources>,
    mut images: ResMut<Assets<Image>>,
) {
    if images.get(&render_resources.render_target).is_none() {
        render_resources.render_target = images.add(device.create_image(
            swapchain.width,
            swapchain.height,
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageLayout::GENERAL,
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
        ));
    }
}

fn wait_for_frame_finish(device: Res<RenderDevice>, mut swapchain: ResMut<Swapchain>) {
    unsafe {
        device
            .device
            .wait_for_fences(std::slice::from_ref(&swapchain.fence), true, u64::MAX)
            .unwrap();
        device
            .device
            .reset_fences(std::slice::from_ref(&swapchain.fence))
            .unwrap();

        // get the next image to render to
        swapchain.aquire_next_image(&device);
    }
}

fn render(
    device: Res<RenderDevice>,
    mut swapchain: ResMut<Swapchain>,
    render_resources: Res<RenderResources>,
    rt_pipelines: Res<VulkanAssets<RaytracingPipeline>>,
    rast_pipelines: Res<VulkanAssets<RasterizationPipeline>>,
    images: Res<Assets<Image>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
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

        let render_target = images
            .get(&render_resources.render_target)
            .expect("render target not found");

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

        // Make swapchain available for rendering
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            swapchain_image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
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

        device
            .device
            .queue_submit(
                device.queue,
                std::slice::from_ref(&submit_info),
                swapchain.fence,
            )
            .unwrap();

        let image_idx = swapchain.current_image_idx as u32;

        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(std::slice::from_ref(&swapchain.render_finished_sem))
            .swapchains(std::slice::from_ref(&swapchain.handle))
            .image_indices(std::slice::from_ref(&image_idx))
            .build();

        let present_result = device
            .exts
            .swapchain
            .queue_present(device.queue, &present_info);

        match present_result {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::SUBOPTIMAL_KHR) => {
                let primary_window = primary_window.get_single().unwrap();
                swapchain.on_resize(&device, primary_window);
            }
            Err(e) => panic!("Failed to present swapchain image: {:?}", e),
            Ok(_) => {}
        }
    }
}
