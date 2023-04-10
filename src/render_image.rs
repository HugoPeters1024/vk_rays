use ash::vk;
use bevy::reflect::TypeUuid;
use gpu_allocator::vulkan::*;
use gpu_allocator::*;

use crate::render_device::RenderDevice;
use crate::vk_utils;

#[derive(TypeUuid)]
#[uuid = "f5b5b0f0-1b5f-4b0e-9c1f-1f1b0c0c0c2d"]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: vk::Format,
    pub usage: vk::ImageUsageFlags,
    pub handle: vk::Image,
    pub view: vk::ImageView,
    pub allocation: Allocation,
}

pub trait ImageProvider {
    fn create_image(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        initial_layout: vk::ImageLayout,
        usage: vk::ImageUsageFlags,
    ) -> Image;
}

impl ImageProvider for RenderDevice {
    fn create_image(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        initial_layout: vk::ImageLayout,
        usage: vk::ImageUsageFlags,
    ) -> Image {
        println!(
            "Allocating an image of type {:?} and size {}x{}",
            format, width, height
        );
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
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let handle = unsafe { self.device.create_image(&image_info, None).unwrap() };

        let requirements = unsafe { self.device.get_image_memory_requirements(handle) };
        let allocation = self
            .alloc_impl
            .lock()
            .unwrap()
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
            self.device
                .bind_image_memory(handle, allocation.memory(), allocation.offset())
                .unwrap();
        }

        let view_info = crate::initializers::image_view_info(handle.clone(), format);
        let view = unsafe { self.device.create_image_view(&view_info, None).unwrap() };

        unsafe {
            self.run_single_commands(&|command_buffer| {
                vk_utils::transition_image_layout(
                    self,
                    command_buffer,
                    handle,
                    vk::ImageLayout::UNDEFINED,
                    initial_layout,
                );
            });
        }

        Image {
            handle,
            view,
            allocation,
            width,
            height,
            format,
            usage,
        }
    }
}
