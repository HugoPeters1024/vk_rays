use crate::{
    initializers, render_buffer::BufferProvider, render_device::RenderDevice, render_image::VkImage, vk_utils,
    vulkan_assets::VulkanAsset,
};
use ash::vk;
use gpu_allocator::{
    vulkan::{AllocationCreateDesc, AllocationScheme},
    MemoryLocation,
};

impl VulkanAsset for bevy::prelude::Image {
    type ExtractedAsset = bevy::prelude::Image;
    type PreparedAsset = VkImage;
    type ExtractParam = ();

    fn extract_asset(
        &self,
        _param: &mut bevy::ecs::system::SystemParamItem<Self::ExtractParam>,
    ) -> Option<Self::ExtractedAsset> {
        Some(self.clone())
    }

    fn prepare_asset(device: &crate::render_device::RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        load_texture_from_bytes(
            device,
            vk::Format::R32G32B32A32_SFLOAT,
            &asset.data,
            asset.texture_descriptor.size.width,
            asset.texture_descriptor.size.height,
        )
    }

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &crate::vulkan_cleanup::VkCleanup) {
        cleanup.send(crate::vulkan_cleanup::VkCleanupEvent::ImageView(asset.view));
        cleanup.send(crate::vulkan_cleanup::VkCleanupEvent::Image(asset.handle));
    }
}

pub fn load_texture_from_bytes(
    device: &RenderDevice,
    format: vk::Format,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> VkImage {
    let target_bytes_per_pixel = match format {
        vk::Format::R8G8B8A8_UNORM => 4,
        vk::Format::R32G32B32A32_SFLOAT => 16,
        _ => panic!("unsupported format"),
    };

    assert!(
        bytes.len() == (width * height) as usize * target_bytes_per_pixel,
        "expected {} bytes, got {}",
        (width * height) as usize * target_bytes_per_pixel,
        bytes.len()
    );
    let mut staging_buffer = device.create_host_buffer::<u8>(
        (width * height * target_bytes_per_pixel as u32) as u64,
        vk::BufferUsageFlags::TRANSFER_SRC,
    );
    {
        let mut staging_buffer = device.map_buffer(&mut staging_buffer);
        staging_buffer.as_slice_mut().copy_from_slice(bytes);
    }

    let image_info = vk::ImageCreateInfo::builder()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);

    let image_handle = unsafe { device.device.create_image(&image_info, None).unwrap() };

    let requirements = unsafe { device.device.get_image_memory_requirements(image_handle) };

    {
        let mut alloc_impl = device.write_alloc();

        let allocation = alloc_impl
            .allocator
            .allocate(&AllocationCreateDesc {
                name: "",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::DedicatedImage(image_handle),
            })
            .unwrap();

        unsafe {
            device
                .device
                .bind_image_memory(image_handle, allocation.memory(), allocation.offset())
                .unwrap();
        }

        alloc_impl.image_to_allocation.insert(image_handle, allocation);
    }

    device.run_asset_commands(|cmd_buffer| {
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            image_handle,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
        let copy_region = initializers::buffer_image_copy(width, height);
        unsafe {
            device.device.cmd_copy_buffer_to_image(
                cmd_buffer,
                staging_buffer.handle,
                image_handle,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                std::slice::from_ref(&copy_region),
            );
        };
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            image_handle,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );
    });

    device.destroy_buffer(staging_buffer);

    let view_info = crate::initializers::image_view_info(image_handle.clone(), format);
    let view = unsafe { device.device.create_image_view(&view_info, None).unwrap() };

    VkImage {
        handle: image_handle,
        view,
    }
}

pub fn padd_pixel_bytes_rgba_unorm(bytes: &[u8], src_bytes_per_pixel: u32, width: usize, height: usize) -> Vec<u8> {
    let mut padded_bytes = vec![0u8; (width * height * 4) as usize];

    for pixel_idx in 0..width * height {
        for channel_idx in 0..4 {
            if channel_idx < src_bytes_per_pixel {
                padded_bytes[pixel_idx * 4 + channel_idx as usize] =
                    bytes[pixel_idx * src_bytes_per_pixel as usize + channel_idx as usize];
            } else {
                // padd alpha white, color black
                padded_bytes[pixel_idx * 4 + channel_idx as usize] = if channel_idx == 3 { 255 } else { 0 };
            }
        }
    }

    padded_bytes
}
