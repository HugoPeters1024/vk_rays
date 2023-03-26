use ash::vk;

pub fn image_view_info(image: vk::Image, format: vk::Format) -> vk::ImageViewCreateInfo {
    vk::ImageViewCreateInfo::builder()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(
            vk::ImageSubresourceRange::builder()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1)
                .build(),
        )
        .build()
}

pub fn layout_transition2(image: vk::Image, from: vk::ImageLayout, to: vk::ImageLayout) -> vk::ImageMemoryBarrier2 {
    vk::ImageMemoryBarrier2::builder()
        .image(image.clone())
        .old_layout(from)
        .new_layout(to)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })
        .build()
}

