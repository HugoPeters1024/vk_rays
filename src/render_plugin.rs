use crate::swapchain;
use crate::{render_device::RenderDevice, swapchain::Swapchain};
use ash::vk;
use bevy::{
    ecs::system::SystemState,
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper},
};

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_asset::<crate::shader::Shader>()
            .init_asset_loader::<crate::shader::ShaderLoader>()
            .init_debug_asset_loader::<crate::shader::ShaderLoader>();

        let mut system_state: SystemState<Query<&RawHandleWrapper, With<PrimaryWindow>>> =
            SystemState::new(&mut app.world);
        let query = system_state.get(&app.world);

        let whandles = query.get_single().unwrap();
        let render_device = RenderDevice::from_window(whandles);
        app.world.insert_resource(render_device);

        app.add_plugin(swapchain::SwapchainPlugin);

        app.add_system(render);
    }
}

fn render(world: &mut World) {
    let mut params: SystemState<(
        Res<RenderDevice>,
        ResMut<Swapchain>,
        Query<&Window, With<PrimaryWindow>>,
    )> = SystemState::new(world);
    let (device, mut swapchain, primary_window) = params.get_mut(world);

    // wait for the previous frame to finish
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
        let (image, view) = swapchain.current_framebuffer();

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
        let image_barrier = crate::initializers::layout_transition2(
            image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::PRESENT_SRC_KHR,
        );
        let barrier_info = vk::DependencyInfo::builder()
            .image_memory_barriers(std::slice::from_ref(&image_barrier));
        device
            .exts
            .sync2
            .cmd_pipeline_barrier2(cmd_buffer, &barrier_info);
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
