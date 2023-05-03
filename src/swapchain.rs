use crate::{
    render_device::RenderDevice,
    vulkan_cleanup::{VkCleanup, VkCleanupEvent}, render_image::{Image, vk_image_from_asset, VkImage}, vk_utils,
};
use ash::vk;
use bevy::{
    ecs::system::SystemState,
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper},
};

pub struct SwapchainPlugin;

impl Plugin for SwapchainPlugin {
    fn build(&self, app: &mut App) {
        let mut system_state: SystemState<Query<(Entity, &Window, &RawHandleWrapper), With<PrimaryWindow>>> =
            SystemState::new(&mut app.world);
        let query = system_state.get(&app.world);
        let (primary_window_e, primary_window, whandles) = query.get_single().unwrap();

        let render_device = app.world.get_resource::<RenderDevice>().unwrap();
        let cleanup = app.world.get_resource::<VkCleanup>().unwrap();
        let swapchain = Swapchain::new(render_device.clone(), cleanup.clone(), whandles, primary_window);

        app.world.entity_mut(primary_window_e).insert(swapchain);
    }
}

#[derive(Component)]
pub struct Swapchain {
    cleanup: VkCleanup,
    device: RenderDevice,
    pub surface: vk::SurfaceKHR,
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub width: u32,
    pub height: u32,
    pub image_ready_sem: vk::Semaphore,
    pub render_finished_sem: vk::Semaphore,
    pub fence: vk::Fence,
    pub current_image_idx: usize,
    pub render_target: VkImage,
}

impl Swapchain {
    pub fn new(device: RenderDevice, cleanup: VkCleanup, whandles: &RawHandleWrapper, window: &Window) -> Self {
        unsafe {
            let surface = device.create_surface(whandles);
            let semaphore_info = vk::SemaphoreCreateInfo::builder();
            let image_ready_sem = device.device.create_semaphore(&semaphore_info, None).unwrap();
            let render_finished_sem = device.device.create_semaphore(&semaphore_info, None).unwrap();

            let fence_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
            let fence = device.device.create_fence(&fence_info, None).unwrap();

            let mut ret = Self {
                cleanup,
                device,
                surface,
                handle: vk::SwapchainKHR::null(),
                images: Vec::new(),
                views: Vec::new(),
                width: 0,
                height: 0,
                image_ready_sem,
                render_finished_sem,
                fence,
                current_image_idx: 0,
                render_target: VkImage::null(),
            };

            ret.on_resize(window);
            ret
        }
    }

    pub fn current_framebuffer(&self) -> (vk::Image, vk::ImageView) {
        (self.images[self.current_image_idx], self.views[self.current_image_idx])
    }

    pub fn aquire_next_image(&mut self, device: &RenderDevice) {
        let result = unsafe {
            device
                .exts
                .swapchain
                .acquire_next_image(self.handle, u64::MAX, self.image_ready_sem, vk::Fence::null())
        }
        .unwrap();
        self.current_image_idx = result.0 as usize;
    }

    pub unsafe fn on_resize(&mut self, window: &Window) {
        let surface_format = self
            .device
            .exts
            .surface
            .get_physical_device_surface_formats(self.device.physical_device, self.surface)
            .unwrap()[0];
        let surface_caps = self
            .device
            .exts
            .surface
            .get_physical_device_surface_capabilities(self.device.physical_device, self.surface)
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
        let present_modes = self
            .device
            .exts
            .surface
            .get_physical_device_surface_present_modes(self.device.physical_device, self.surface)
            .unwrap();

        let present_mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);

        let old_swapchain = self.handle;
        let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(self.surface)
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
            .old_swapchain(old_swapchain);

        self.handle = self
            .device
            .exts
            .swapchain
            .create_swapchain(&swapchain_create_info, None)
            .unwrap();

        // Cleanup old swapchain
        for view in self.views.iter() {
            self.cleanup.send(VkCleanupEvent::ImageView(*view));
        }
        self.cleanup.send(VkCleanupEvent::Swapchain(old_swapchain));

        self.images = self.device.exts.swapchain.get_swapchain_images(self.handle).unwrap();

        self.views = self
            .images
            .iter()
            .map(|image| {
                let view_info = crate::initializers::image_view_info(image.clone(), surface_format.format);
                self.device.device.create_image_view(&view_info, None).unwrap()
            })
            .collect();

        self.cleanup.send(VkCleanupEvent::ImageView(self.render_target.view));
        self.cleanup.send(VkCleanupEvent::Image(self.render_target.handle));

        self.render_target = vk_image_from_asset(
            &self.device,
            &Image {
                width: self.width,
                height: self.height,
                format: vk::Format::R32G32B32A32_SFLOAT,
                usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
                initial_layout: vk::ImageLayout::UNDEFINED,
            },
        );

        self.device.run_single_commands(&|cmd_buffer| {
            vk_utils::transition_image_layout(
                &self.device,
                cmd_buffer,
                self.render_target.handle,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::GENERAL,
            );
        });

        println!("Swapchain Resized: {}x{}", self.width, self.height);
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        println!("Swapchain is being dropped");
        self.device.wait_idle();
        let dv = &self.device.device;
        unsafe {
            dv.destroy_image_view(self.render_target.view, None);
            dv.destroy_image(self.render_target.handle, None);
            dv.destroy_fence(self.fence, None);
            dv.destroy_semaphore(self.render_finished_sem, None);
            dv.destroy_semaphore(self.image_ready_sem, None);
            for view in self.views.iter() {
                dv.destroy_image_view(*view, None);
            }
            self.device.exts.swapchain.destroy_swapchain(self.handle, None);

            self.device.exts.surface.destroy_surface(self.surface, None);
        }
        println!("Swapchain dropped");
    }
}
