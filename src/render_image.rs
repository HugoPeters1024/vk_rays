use ash::vk;
use bevy::reflect::TypeUuid;
use gpu_allocator::vulkan::*;
use gpu_allocator::*;

use crate::render_device::RenderDevice;
use crate::vk_utils;
use crate::vulkan_assets::VulkanAsset;
use crate::vulkan_cleanup::{VkCleanup, VkCleanupEvent};

#[derive(TypeUuid, Clone)]
#[uuid = "f5b5b0f0-1b5f-4b0e-9c1f-1f1b0c0c0c2d"]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: vk::Format,
    pub usage: vk::ImageUsageFlags,
    pub initial_layout: vk::ImageLayout,
}

#[derive(TypeUuid)]
#[uuid = "3785ec50-3fc4-495e-908e-ad68893f48f7"]
pub struct VkImage {
    pub handle: vk::Image,
    pub view: vk::ImageView,
}

impl VulkanAsset for Image {
    type ExtractedAsset = Image;
    type PreparedAsset = VkImage;

    type Param = ();

    fn extract_asset(
        &self,
        _param: &mut bevy::ecs::system::SystemParamItem<Self::Param>,
    ) -> Option<Self::ExtractedAsset> {
        Some(self.clone())
    }

    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        println!(
            "Allocating an image of type {:?} and size {}x{}",
            asset.format, asset.width, asset.height
        );
        let image_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(asset.format)
            .extent(vk::Extent3D {
                width: asset.width,
                height: asset.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(asset.usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let handle = unsafe { device.device.create_image(&image_info, None).unwrap() };

        let requirements = unsafe { device.device.get_image_memory_requirements(handle) };

        {
            let mut alloc_impl = device.write_alloc();

            let allocation = alloc_impl
                .allocator
                .allocate(&AllocationCreateDesc {
                    name: "",
                    requirements,
                    location: MemoryLocation::GpuOnly,
                    linear: false,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .unwrap();


            unsafe {
                device.device
                    .bind_image_memory(handle, allocation.memory(), allocation.offset())
                    .unwrap();
            }

            alloc_impl.image_to_allocation.insert(handle, allocation);
        }

        let view_info = crate::initializers::image_view_info(handle.clone(), asset.format);
        let view = unsafe { device.device.create_image_view(&view_info, None).unwrap() };

        unsafe {
            device.run_single_commands(&|command_buffer| {
                vk_utils::transition_image_layout(
                    device,
                    command_buffer,
                    handle,
                    vk::ImageLayout::UNDEFINED,
                    asset.initial_layout,
                );
            });
        }

        VkImage {
            handle,
            view,
        }
    }

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &VkCleanup) {
        cleanup.send(VkCleanupEvent::ImageView(asset.view));
        cleanup.send(VkCleanupEvent::Image(asset.handle));
    }

}
