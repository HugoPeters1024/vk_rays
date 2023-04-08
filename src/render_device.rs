use ash::extensions::khr;
use ash::{vk, Device, Entry, Instance};
use bevy::prelude::*;
use bevy::window::RawHandleWrapper;
use gpu_allocator::vulkan::*;
use std::ffi::{c_char, CStr};
use std::sync::Mutex;

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
    pub descriptor_pool: vk::DescriptorPool,
    pub cmd_buffer: vk::CommandBuffer,
    pub allocator: Mutex<Allocator>,
    pub single_time_command_buffer: vk::CommandBuffer,
    pub single_time_fence: vk::Fence,
}

pub struct Exts {
    pub surface: khr::Surface,
    pub swapchain: khr::Swapchain,
    pub sync2: khr::Synchronization2,
    pub rt_pipeline: khr::RayTracingPipeline,
    pub rt_acc_struct: khr::AccelerationStructure,
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

            let all_devices = instance.enumerate_physical_devices().unwrap();
            println!("Available devices:");
            for device in all_devices.iter() {
                let info = instance.get_physical_device_properties(*device);
                println!(
                    "  - {}",
                    CStr::from_ptr(info.device_name.as_ptr()).to_str().unwrap()
                );
            }

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
                khr::Maintenance4::name().as_ptr(),
                khr::AccelerationStructure::name().as_ptr(),
                khr::RayTracingPipeline::name().as_ptr(),
                khr::DeferredHostOperations::name().as_ptr(),
                vk::KhrSpirv14Fn::name().as_ptr(),
                vk::ExtDescriptorIndexingFn::name().as_ptr(),
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

            let mut bda_info = vk::PhysicalDeviceBufferDeviceAddressFeatures::builder()
                .buffer_device_address(true)
                .build();

            let mut maintaince4_info = vk::PhysicalDeviceMaintenance4Features::builder()
                .maintenance4(true)
                .build();

            let mut dynamic_rendering_info = vk::PhysicalDeviceDynamicRenderingFeatures::builder()
                .dynamic_rendering(true)
                .build();

            let mut features_indexing = vk::PhysicalDeviceDescriptorIndexingFeatures::builder()
                .descriptor_binding_partially_bound(true)
                .runtime_descriptor_array(true)
                .descriptor_binding_sampled_image_update_after_bind(true)
                .descriptor_binding_storage_image_update_after_bind(true)
                .descriptor_binding_variable_descriptor_count(true);

            let mut features_acceleration_structure =
                vk::PhysicalDeviceAccelerationStructureFeaturesKHR::builder()
                    .acceleration_structure(true)
                    .build();

            let mut features_raytracing_pipeline =
                vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::builder()
                    .ray_tracing_pipeline(true)
                    .build();

            let device_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(std::slice::from_ref(&queue_info))
                .enabled_extension_names(&device_extensions)
                .push_next(&mut sync2_info)
                .push_next(&mut bda_info)
                .push_next(&mut maintaince4_info)
                .push_next(&mut dynamic_rendering_info)
                .push_next(&mut features_indexing)
                .push_next(&mut features_acceleration_structure)
                .push_next(&mut features_raytracing_pipeline);

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

            let pool_sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1000,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: 1000,
                },
            ];
            let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
                .pool_sizes(&pool_sizes)
                .max_sets(1000);

            let descriptor_pool = device
                .create_descriptor_pool(&descriptor_pool_info, None)
                .unwrap();

            let allocator = Mutex::new(
                Allocator::new(&AllocatorCreateDesc {
                    instance: instance.clone(),
                    device: device.clone(),
                    physical_device: physical_device.clone(),
                    debug_settings: Default::default(),
                    buffer_device_address: true,
                })
                .unwrap(),
            );

            let single_time_command_buffer =
                device.allocate_command_buffers(&alloc_info).unwrap()[0];
            let fence_info = vk::FenceCreateInfo::builder();
            let single_time_fence = device.create_fence(&fence_info, None).unwrap();

            RenderDevice {
                entry,
                exts: Exts {
                    surface: ext_surface,
                    swapchain: khr::Swapchain::new(&instance, &device),
                    sync2: khr::Synchronization2::new(&instance, &device),
                    rt_pipeline: khr::RayTracingPipeline::new(&instance, &device),
                    rt_acc_struct: khr::AccelerationStructure::new(&instance, &device),
                },
                instance,
                surface,
                physical_device,
                device,
                queue_family_idx,
                queue,
                command_pool,
                descriptor_pool,
                cmd_buffer,
                allocator,
                single_time_command_buffer,
                single_time_fence,
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

    pub unsafe fn run_single_commands(&self, f: &dyn Fn(vk::CommandBuffer)) {
        self.device
            .reset_command_buffer(
                self.single_time_command_buffer,
                vk::CommandBufferResetFlags::empty(),
            )
            .unwrap();
        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        self.device
            .begin_command_buffer(self.single_time_command_buffer, &begin_info)
            .unwrap();
        f(self.single_time_command_buffer);
        self.device
            .end_command_buffer(self.single_time_command_buffer)
            .unwrap();

        self.device
            .reset_fences(std::slice::from_ref(&self.single_time_fence))
            .unwrap();
        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(std::slice::from_ref(&self.single_time_command_buffer));

        self.device
            .queue_submit(
                self.queue,
                std::slice::from_ref(&submit_info),
                self.single_time_fence,
            )
            .unwrap();

        self.device
            .wait_for_fences(
                std::slice::from_ref(&self.single_time_fence),
                true,
                u64::MAX,
            )
            .unwrap();
    }
}
