use crate::render_device::RenderDevice;
use ash::vk;
use bevy::{ecs::system::SystemState, prelude::*, window::PrimaryWindow};

pub struct SwapchainPlugin;

impl Plugin for SwapchainPlugin {
    fn build(&self, app: &mut App) {
        let mut system_state: SystemState<Query<&Window, With<PrimaryWindow>>> =
            SystemState::new(&mut app.world);
        let query = system_state.get(&app.world);
        let primary_window = query.get_single().unwrap();

        let render_device = app.world.get_resource::<RenderDevice>().unwrap();
        app.world
            .insert_resource(Swapchain::new(render_device, primary_window));
    }
}

#[derive(Resource)]
pub struct Swapchain {
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub width: u32,
    pub height: u32,
    pub image_ready_sem: vk::Semaphore,
    pub render_finished_sem: vk::Semaphore,
    pub fence: vk::Fence,
    pub current_image_idx: usize,
}

impl Swapchain {
    pub fn new(device: &RenderDevice, window: &Window) -> Self {
        unsafe {
            let semaphore_info = vk::SemaphoreCreateInfo::builder();
            let image_ready_sem = device
                .device
                .create_semaphore(&semaphore_info, None)
                .unwrap();
            let render_finished_sem = device
                .device
                .create_semaphore(&semaphore_info, None)
                .unwrap();

            let fence_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
            let fence = device.device.create_fence(&fence_info, None).unwrap();

            let mut ret = Self {
                handle: vk::SwapchainKHR::null(),
                images: Vec::new(),
                views: Vec::new(),
                width: 0,
                height: 0,
                image_ready_sem,
                render_finished_sem,
                fence,
                current_image_idx: 0,
            };

            ret.on_resize(device, window);
            ret
        }
    }

    pub fn current_framebuffer(&self) -> (vk::Image, vk::ImageView) {
        (
            self.images[self.current_image_idx],
            self.views[self.current_image_idx],
        )
    }

    pub unsafe fn aquire_next_image(&mut self, device: &RenderDevice) {
        let result = device
            .exts
            .swapchain
            .acquire_next_image(
                self.handle,
                u64::MAX,
                self.image_ready_sem,
                vk::Fence::null(),
            )
            .unwrap();

        self.current_image_idx = result.0 as usize;
    }

    pub unsafe fn on_resize(&mut self, device: &RenderDevice, window: &Window) {
        let surface_format = device
            .exts
            .surface
            .get_physical_device_surface_formats(device.physical_device, device.surface)
            .unwrap()[0];
        let surface_caps = device
            .exts
            .surface
            .get_physical_device_surface_capabilities(device.physical_device, device.surface)
            .unwrap();

        let mut desired_image_count = surface_caps.min_image_count + 1;
        if surface_caps.max_image_count > 0 && desired_image_count > surface_caps.max_image_count {
            desired_image_count = surface_caps.max_image_count;
        }

        let surface_resolution = match surface_caps.current_extent.width {
            std::u32::MAX => vk::Extent2D {
                width: window.physical_width(),
                height: window.physical_height(),
            },
            _ => surface_caps.current_extent,
        };

        self.width = surface_resolution.width;
        self.height = surface_resolution.height;

        let pre_transform = if surface_caps
            .supported_transforms
            .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
            vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
            surface_caps.current_transform
        };
        let present_modes = device
            .exts
            .surface
            .get_physical_device_surface_present_modes(device.physical_device, device.surface)
            .unwrap();

        let present_mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(device.surface)
            .min_image_count(desired_image_count)
            .image_color_space(surface_format.color_space)
            .image_format(surface_format.format)
            .image_extent(surface_resolution)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(pre_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .image_array_layers(1)
            .old_swapchain(self.handle);

        self.handle = device
            .exts
            .swapchain
            .create_swapchain(&swapchain_create_info, None)
            .unwrap();

        self.images = device
            .exts
            .swapchain
            .get_swapchain_images(self.handle)
            .unwrap();
        self.views = self
            .images
            .iter()
            .map(|image| {
                let view_info =
                    crate::initializers::image_view_info(image.clone(), surface_format.format);
                device.device.create_image_view(&view_info, None).unwrap()
            })
            .collect();

        println!("Swapchain Resized: {}x{}", self.width, self.height);
    }
}
