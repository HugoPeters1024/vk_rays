use ash::vk;

use crate::render_device::RenderDevice;

pub fn aligned_size(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}

pub fn transition_image_layout(
    device: &RenderDevice,
    cmd_buffer: vk::CommandBuffer,
    image: vk::Image,
    from: vk::ImageLayout,
    to: vk::ImageLayout,
) {
    let image_barrier = crate::initializers::layout_transition2(image, from, to);

    let barrier_info =
        vk::DependencyInfo::builder().image_memory_barriers(std::slice::from_ref(&image_barrier));

    unsafe {
        device
            .exts
            .sync2
            .cmd_pipeline_barrier2(cmd_buffer, &barrier_info);
    }
}
