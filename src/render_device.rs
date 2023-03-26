use ash::extensions::khr;
use ash::vk::{ExtDescriptorIndexingFn, KhrMaintenance4Fn, KhrSpirv14Fn};
use ash::{vk, Device, Entry, Instance};
use bevy::prelude::*;
use bevy::window::RawHandleWrapper;
use gpu_allocator::vulkan::*;
use std::ffi::{c_char, CStr};

#[derive(Resource)]
pub struct RenderDevice {
    pub entry: Entry,
    pub exts: Exts,
    pub instance: Instance,
    pub surface: vk::SurfaceKHR,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue_family_idx: u32,
    pub queue: vk::Queue,
    pub command_pool: vk::CommandPool,
    pub cmd_buffer: vk::CommandBuffer,
}

pub struct Exts {
    pub surface: khr::Surface,
    pub swapchain: khr::Swapchain,
    pub sync2: khr::Synchronization2,
}

impl RenderDevice {
    pub fn from_window(window: &RawHandleWrapper) -> Self {
        unsafe {
            let entry = Entry::load().unwrap();
            let app_name = CStr::from_bytes_with_nul_unchecked(b"VK RAYS\0");

            let mut layer_names: Vec<&CStr> = Vec::new();

            #[cfg(debug_assertions)]
            layer_names.push(CStr::from_bytes_with_nul_unchecked(
                b"VK_LAYER_KHRONOS_validation\0",
            ));

            println!("Validation layers:");
            for layer_name in layer_names.iter() {
                println!("  - {}", layer_name.to_str().unwrap());
            }

            let layers_names_raw: Vec<*const c_char> = layer_names
                .iter()
                .map(|raw_name| raw_name.as_ptr())
                .collect();

            let instance_extensions =
                ash_window::enumerate_required_extensions(window.display_handle).unwrap();

            println!("Instance extensions:");
            for extension_name in instance_extensions.iter() {
                println!("  - {}", CStr::from_ptr(*extension_name).to_str().unwrap());
            }

            let app_info = vk::ApplicationInfo::builder()
                .application_name(app_name)
                .application_version(0)
                .engine_name(app_name)
                .engine_version(0)
                .api_version(vk::make_api_version(0, 1, 3, 0));

            let instance_info = vk::InstanceCreateInfo::builder()
                .application_info(&app_info)
                .enabled_layer_names(&layers_names_raw)
                .enabled_extension_names(&instance_extensions);

            let instance = entry.create_instance(&instance_info, None).unwrap();

            let ext_surface = khr::Surface::new(&entry, &instance);
            let surface = ash_window::create_surface(
                &entry,
                &instance,
                window.display_handle,
                window.window_handle,
                None,
            )
            .unwrap();

            let (physical_device, queue_family_idx) = instance
                .enumerate_physical_devices()
                .unwrap()
                .into_iter()
                .find_map(|d| {
                    let info = instance.get_physical_device_properties(d);
                    if !CStr::from_ptr(info.device_name.as_ptr())
                        .to_str()
                        .unwrap()
                        .contains("NVIDIA")
                    {
                        return None;
                    }

                    let properties = instance.get_physical_device_queue_family_properties(d);
                    properties.iter().enumerate().find_map(|(i, p)| {
                        if p.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                            && ext_surface
                                .get_physical_device_surface_support(d, i as u32, surface)
                                .unwrap()
                        {
                            Some((d, i as u32))
                        } else {
                            None
                        }
                    })
                })
                .unwrap();

            let device_properties = instance.get_physical_device_properties(physical_device);
            println!(
                "Running on device: {}",
                CStr::from_ptr(device_properties.device_name.as_ptr())
                    .to_str()
                    .unwrap()
            );

            let device_extensions = [
                khr::Swapchain::name().as_ptr(),
                khr::Synchronization2::name().as_ptr(),
            ];
            println!("Device extensions:");
            for extension_name in device_extensions.iter() {
                println!("  - {}", CStr::from_ptr(*extension_name).to_str().unwrap());
            }

            let queue_info = vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(queue_family_idx)
                .queue_priorities(&[1.0])
                .build();

            let mut sync2_info = vk::PhysicalDeviceSynchronization2Features::builder()
                .synchronization2(true)
                .build();

            let device_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(std::slice::from_ref(&queue_info))
                .enabled_extension_names(&device_extensions)
                .push_next(&mut sync2_info);

            let device = instance
                .create_device(physical_device, &device_info, None)
                .unwrap();
            let queue = device.get_device_queue(queue_family_idx, 0);

            let pool_info = vk::CommandPoolCreateInfo::builder()
                .queue_family_index(queue_family_idx)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

            let command_pool = device.create_command_pool(&pool_info, None).unwrap();
            let alloc_info = vk::CommandBufferAllocateInfo::builder()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let cmd_buffer = device.allocate_command_buffers(&alloc_info).unwrap()[0];

            RenderDevice {
                entry,
                exts: Exts {
                    surface: ext_surface,
                    swapchain: khr::Swapchain::new(&instance, &device),
                    sync2: khr::Synchronization2::new(&instance, &device),
                },
                instance,
                surface,
                physical_device,
                device,
                queue_family_idx,
                queue,
                command_pool,
                cmd_buffer,
            }
        }
    }
    pub fn device_name(&self) -> String {
        unsafe {
            let device_properties = self
                .instance
                .get_physical_device_properties(self.physical_device);
            CStr::from_ptr(device_properties.device_name.as_ptr())
                .to_str()
                .unwrap()
                .to_string()
        }
    }
}
