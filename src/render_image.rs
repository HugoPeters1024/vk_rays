use ash::vk;
use gpu_allocator::vulkan::*;
use gpu_allocator::*;

use crate::render_device::RenderDevice;

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
        device: &RenderDevice,
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
        device: &RenderDevice,
        width: u32,
        height: u32,
        format: vk::Format,
        initial_layout: vk::ImageLayout,
        usage: vk::ImageUsageFlags,
    ) -> Image {
        println!("Allocating an image of type {:?} and size {}x{}", format, width, height);
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
        let handle = unsafe { device.device.create_image(&image_info, None).unwrap() };

        let requirements = unsafe { device.device.get_image_memory_requirements(handle) };
        let allocation = self
            .allocator
            .lock()
            .unwrap()
            .allocate(&AllocationCreateDesc {
                name: "",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .unwrap();

        unsafe {
            device
                .device
                .bind_image_memory(handle, allocation.memory(), allocation.offset())
                .unwrap();
        }

        let view_info = crate::initializers::image_view_info(handle.clone(), format);
        let view = unsafe { device.device.create_image_view(&view_info, None).unwrap() };

        unsafe {
            device.run_single_commands(&|command_buffer| {
                let barrier =
                    crate::initializers::layout_transition2(handle, vk::ImageLayout::UNDEFINED, initial_layout);
                let dependency_info = vk::DependencyInfo::builder()
                    .image_memory_barriers(std::slice::from_ref(&barrier))
                    .build();
                device.device.cmd_pipeline_barrier2(command_buffer, &dependency_info);
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
