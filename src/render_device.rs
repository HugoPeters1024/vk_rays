use ash::extensions::khr;
use ash::vk::Handle;
use ash::{vk, Device, Entry, Instance};
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy::window::RawHandleWrapper;
use gpu_allocator::vulkan::*;
use gpu_allocator::AllocatorDebugSettings;
use std::ffi::{c_char, CStr};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

const MAX_BINDLESS_IMAGES: u32 = 16536;
const BINDLESS_IMAGES_BINDING: u32 = 16;

#[derive(Resource, Clone, Deref)]
pub struct RenderDevice(Arc<RenderDeviceImpl>);

impl RenderDevice {
    pub fn from_window(window: &RawHandleWrapper) -> Self {
        let device = Arc::new(RenderDeviceImpl::from_window(window));
        Self(device)
    }
}

pub struct AllocImpl {
    pub allocator: Allocator,
    pub buffer_to_allocation: HashMap<vk::Buffer, Allocation>,
    pub image_to_allocation: HashMap<vk::Image, Allocation>,
}

pub struct GDescriptorMap {
    pub g_descriptor_map: HashMap<vk::ImageView, u32>,
    pub g_descriptor_idx_gen: u32,
}

impl Drop for AllocImpl {
    fn drop(&mut self) {
        if !self.buffer_to_allocation.is_empty() {
            println!("Some buffers were not freed");
        }
        if !self.image_to_allocation.is_empty() {
            println!("Some images were not freed");
        }
    }
}

pub struct RenderDeviceImpl {
    pub entry: Entry,
    pub exts: Exts,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue_family_idx: u32,
    pub queue: Arc<Mutex<vk::Queue>>,
    pub command_pool: vk::CommandPool,
    pub g_descriptor_set_layout: vk::DescriptorSetLayout,
    pub g_descriptor_set: vk::DescriptorSet,
    pub g_descriptors: Mutex<GDescriptorMap>,
    pub asset_command_pool: Mutex<vk::CommandPool>,
    pub descriptor_pool: vk::DescriptorPool,
    pub single_time_command_buffer: vk::CommandBuffer,
    pub single_time_fence: vk::Fence,
    pub nearest_sampler: vk::Sampler,
    pub linear_sampler: vk::Sampler,
    pub alloc: Option<RwLock<AllocImpl>>,
}

pub struct Exts {
    pub surface: khr::Surface,
    pub swapchain: khr::Swapchain,
    pub sync2: khr::Synchronization2,
    pub rt_pipeline: khr::RayTracingPipeline,
    pub rt_acc_struct: khr::AccelerationStructure,
}

impl RenderDeviceImpl {
    pub fn from_window(window: &RawHandleWrapper) -> Self {
        unsafe {
            let entry = Entry::load().unwrap();
            let app_name = CStr::from_bytes_with_nul_unchecked(b"VK RAYS\0");

            let mut layer_names: Vec<&CStr> = Vec::new();

            #[cfg(debug_assertions)]
            layer_names.push(CStr::from_bytes_with_nul_unchecked(b"VK_LAYER_KHRONOS_validation\0"));

            println!("Validation layers:");
            for layer_name in layer_names.iter() {
                println!("  - {}", layer_name.to_str().unwrap());
            }

            let layers_names_raw: Vec<*const c_char> = layer_names.iter().map(|raw_name| raw_name.as_ptr()).collect();

            let instance_extensions = ash_window::enumerate_required_extensions(window.display_handle).unwrap();

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
            let surface =
                ash_window::create_surface(&entry, &instance, window.display_handle, window.window_handle, None)
                    .unwrap();

            let all_devices = instance.enumerate_physical_devices().unwrap();
            println!("Available devices:");
            for device in all_devices.iter() {
                let info = instance.get_physical_device_properties(*device);
                println!("  - {}", CStr::from_ptr(info.device_name.as_ptr()).to_str().unwrap());
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

            ext_surface.destroy_surface(surface, None);

            let device_properties = instance.get_physical_device_properties(physical_device);
            println!(
                "Running on device: {}",
                CStr::from_ptr(device_properties.device_name.as_ptr()).to_str().unwrap()
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

            let mut features_acceleration_structure = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::builder()
                .acceleration_structure(true)
                .build();

            let mut features_raytracing_pipeline = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::builder()
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

            let device = instance.create_device(physical_device, &device_info, None).unwrap();
            let queue = device.get_device_queue(queue_family_idx, 0);

            let pool_info = vk::CommandPoolCreateInfo::builder()
                .queue_family_index(queue_family_idx)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

            let command_pool = device.create_command_pool(&pool_info, None).unwrap();
            let asset_command_pool = device.create_command_pool(&pool_info, None).unwrap();
            let alloc_info = vk::CommandBufferAllocateInfo::builder()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);

            let bindless_flags: vk::DescriptorBindingFlags = vk::DescriptorBindingFlags::PARTIALLY_BOUND
                | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
                | vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT;
            let max_binding = MAX_BINDLESS_IMAGES - 1;

            let pool_sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1000,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: MAX_BINDLESS_IMAGES,
                },
            ];
            let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
                .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
                .pool_sizes(&pool_sizes)
                .max_sets(1000);

            let descriptor_pool = device.create_descriptor_pool(&descriptor_pool_info, None).unwrap();

            let g_bindless_image_binding = vk::DescriptorSetLayoutBinding::builder()
                .binding(BINDLESS_IMAGES_BINDING)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(MAX_BINDLESS_IMAGES)
                .stage_flags(vk::ShaderStageFlags::ALL)
                .build();

            let mut g_bindless_image_layout_info_ext = vk::DescriptorSetLayoutBindingFlagsCreateInfo::builder()
                .binding_flags(std::slice::from_ref(&bindless_flags));

            let g_descriptor_set_layout_info = vk::DescriptorSetLayoutCreateInfo::builder()
                .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
                .bindings(std::slice::from_ref(&g_bindless_image_binding))
                .push_next(&mut g_bindless_image_layout_info_ext);

            let g_descriptor_set_layout = device
                .create_descriptor_set_layout(&g_descriptor_set_layout_info, None)
                .unwrap();

            let mut g_descriptor_set_alloc_info_ext = vk::DescriptorSetVariableDescriptorCountAllocateInfo::builder()
                .descriptor_counts(std::slice::from_ref(&max_binding));

            let g_descriptor_set_alloc_info = vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(descriptor_pool)
                .set_layouts(std::slice::from_ref(&g_descriptor_set_layout))
                .push_next(&mut g_descriptor_set_alloc_info_ext);

            let g_descriptor_set = device.allocate_descriptor_sets(&g_descriptor_set_alloc_info).unwrap()[0];

            let g_descriptor_map = GDescriptorMap {
                g_descriptor_map: HashMap::new(),
                g_descriptor_idx_gen: 0,
            };

            let single_time_command_buffer = device.allocate_command_buffers(&alloc_info).unwrap()[0];
            let fence_info = vk::FenceCreateInfo::builder();

            let single_time_fence = device.create_fence(&fence_info, None).unwrap();
            let nearest_sampler_info = vk::SamplerCreateInfo::builder()
                .mag_filter(vk::Filter::NEAREST)
                .min_filter(vk::Filter::NEAREST)
                .address_mode_u(vk::SamplerAddressMode::REPEAT)
                .address_mode_v(vk::SamplerAddressMode::REPEAT)
                .address_mode_w(vk::SamplerAddressMode::REPEAT)
                .anisotropy_enable(false)
                .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
                .unnormalized_coordinates(false)
                .mipmap_mode(vk::SamplerMipmapMode::NEAREST);
            let nearest_sampler = device.create_sampler(&nearest_sampler_info, None).unwrap();

            let linear_sampler_info = vk::SamplerCreateInfo::builder()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::REPEAT)
                .address_mode_v(vk::SamplerAddressMode::REPEAT)
                .address_mode_w(vk::SamplerAddressMode::REPEAT)
                .anisotropy_enable(false)
                .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
                .unnormalized_coordinates(false)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
            let linear_sampler = device.create_sampler(&linear_sampler_info, None).unwrap();

            let alloc = Some(RwLock::new(AllocImpl {
                allocator: Allocator::new(&AllocatorCreateDesc {
                    instance: instance.clone(),
                    device: device.clone(),
                    physical_device: physical_device.clone(),
                    debug_settings: AllocatorDebugSettings {
                        log_leaks_on_shutdown: false,
                        ..default()
                    },
                    buffer_device_address: true,
                })
                .unwrap(),
                buffer_to_allocation: HashMap::new(),
                image_to_allocation: HashMap::new(),
            }));

            Self {
                entry,
                exts: Exts {
                    surface: ext_surface,
                    swapchain: khr::Swapchain::new(&instance, &device),
                    sync2: khr::Synchronization2::new(&instance, &device),
                    rt_pipeline: khr::RayTracingPipeline::new(&instance, &device),
                    rt_acc_struct: khr::AccelerationStructure::new(&instance, &device),
                },
                instance,
                physical_device,
                device,
                queue_family_idx,
                queue: Arc::new(Mutex::new(queue)),
                command_pool,
                g_descriptor_set_layout,
                g_descriptor_set,
                g_descriptors: Mutex::new(g_descriptor_map),
                asset_command_pool: Mutex::new(asset_command_pool),
                descriptor_pool,
                single_time_command_buffer,
                single_time_fence,
                nearest_sampler,
                linear_sampler,
                alloc,
            }
        }
    }

    #[allow(unused)]
    pub fn device_name(&self) -> String {
        unsafe {
            let device_properties = self.instance.get_physical_device_properties(self.physical_device);
            CStr::from_ptr(device_properties.device_name.as_ptr())
                .to_str()
                .unwrap()
                .to_string()
        }
    }

    pub fn read_alloc(&self) -> RwLockReadGuard<AllocImpl> {
        self.alloc.as_ref().unwrap().read().unwrap()
    }

    pub fn write_alloc(&self) -> RwLockWriteGuard<AllocImpl> {
        self.alloc.as_ref().unwrap().write().unwrap()
    }

    pub fn run_asset_commands(&self, f: impl FnOnce(vk::CommandBuffer)) {
        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { self.device.create_fence(&fence_info, None) }.unwrap();
        let asset_command_pool = self.asset_command_pool.lock().unwrap();
        let alloc_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(*asset_command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let cmd_buffer = unsafe { self.device.allocate_command_buffers(&alloc_info) }.unwrap()[0];
        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { self.device.begin_command_buffer(cmd_buffer, &begin_info) }.unwrap();

        f(cmd_buffer);

        unsafe { self.device.end_command_buffer(cmd_buffer) }.unwrap();

        unsafe { self.device.reset_fences(std::slice::from_ref(&fence)) }.unwrap();
        let submit_info = vk::SubmitInfo::builder().command_buffers(std::slice::from_ref(&cmd_buffer));

        {
            let queue = self.queue.lock().unwrap();
            unsafe {
                self.device
                    .queue_submit(queue.clone(), std::slice::from_ref(&submit_info), fence)
            }
            .unwrap();
        }

        unsafe {
            self.device
                .wait_for_fences(std::slice::from_ref(&fence), true, u64::MAX)
        }
        .unwrap();

        unsafe {
            self.device.destroy_fence(fence, None);
        }
    }

    pub unsafe fn run_single_commands(&self, f: &dyn Fn(vk::CommandBuffer)) {
        let queue = self.queue.lock().unwrap();
        self.device
            .reset_command_buffer(self.single_time_command_buffer, vk::CommandBufferResetFlags::empty())
            .unwrap();
        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        self.device
            .begin_command_buffer(self.single_time_command_buffer, &begin_info)
            .unwrap();
        f(self.single_time_command_buffer);
        self.device.end_command_buffer(self.single_time_command_buffer).unwrap();
        self.device
            .reset_fences(std::slice::from_ref(&self.single_time_fence))
            .unwrap();
        let submit_info =
            vk::SubmitInfo::builder().command_buffers(std::slice::from_ref(&self.single_time_command_buffer));
        self.device
            .queue_submit(
                queue.clone(),
                std::slice::from_ref(&submit_info),
                self.single_time_fence,
            )
            .unwrap();
        self.device
            .wait_for_fences(std::slice::from_ref(&self.single_time_fence), true, u64::MAX)
            .unwrap();
    }

    pub fn wait_idle(&self) {
        let queue = self.queue.lock().unwrap();
        unsafe {
            self.device.queue_wait_idle(queue.clone()).unwrap();
        }
    }

    pub fn create_surface(&self, handles: &RawHandleWrapper) -> vk::SurfaceKHR {
        unsafe {
            ash_window::create_surface(
                &self.entry,
                &self.instance,
                handles.display_handle,
                handles.window_handle,
                None,
            )
        }
        .unwrap()
    }

    pub fn get_texture_descriptor_index(&self, view: vk::ImageView) -> u32 {
        let mut g_descriptors = self.g_descriptors.lock().unwrap();
        if let Some(index) = g_descriptors.g_descriptor_map.get(&view) {
            return *index;
        }

        let index = g_descriptors.g_descriptor_idx_gen;
        g_descriptors.g_descriptor_map.insert(view, index);
        g_descriptors.g_descriptor_idx_gen += 1;

        let descriptor_info = vk::DescriptorImageInfo::builder()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(view)
            .sampler(self.linear_sampler);

        let descriptor_write = vk::WriteDescriptorSet::builder()
            .image_info(std::slice::from_ref(&descriptor_info))
            .dst_set(self.g_descriptor_set)
            .dst_array_element(index)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .dst_binding(BINDLESS_IMAGES_BINDING);

        unsafe {
            self.device
                .update_descriptor_sets(std::slice::from_ref(&descriptor_write), &[]);
        }

        index
    }
}

impl Drop for RenderDeviceImpl {
    fn drop(&mut self) {
        println!("RenderDevice is being dropped");
        self.wait_idle();
        let alloc = self.alloc.take().unwrap();
        drop(alloc);
        unsafe {
            {
                let asset_command_pool = self.asset_command_pool.lock().unwrap();
                self.device.destroy_command_pool(*asset_command_pool, None);
            }
            self.device.destroy_fence(self.single_time_fence, None);
            self.device.destroy_sampler(self.nearest_sampler, None);
            self.device.destroy_sampler(self.linear_sampler, None);
            self.device
                .destroy_descriptor_set_layout(self.g_descriptor_set_layout, None);
            self.device.destroy_descriptor_pool(self.descriptor_pool, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
        println!("RenderDevice has been dropped");
    }
}
