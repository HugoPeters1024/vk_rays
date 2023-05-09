use ash::vk;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bytemuck_derive::{Pod, Zeroable};

use crate::composed_asset::{ComposedAsset, ComposedAssetAppExtension};
use crate::render_buffer::{Buffer, BufferProvider};
use crate::render_device::RenderDevice;
use crate::shader::{Shader, ShaderProvider};
use crate::vk_utils;
use crate::vulkan_assets::{AddVulkanAsset, VulkanAsset};
use crate::vulkan_cleanup::{VkCleanup, VkCleanupEvent};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RaytracerRegisters {
    pub uniform_buffer_address: u64,
    pub vertex_buffer_address: u64,
    pub index_buffer_address: u64,
}

#[derive(Default, TypeUuid)]
#[uuid = "a0b0c0d0-e0f0-11ea-87d0-0242ac130003"]
pub struct RaytracingPipeline {
    pub raygen_shader: Handle<Shader>,
    pub hit_shader: Handle<Shader>,
    pub miss_shader: Handle<Shader>,
}

impl ComposedAsset for RaytracingPipeline {
    type DepType = Shader;
    fn get_deps(&self) -> Vec<&Handle<Self::DepType>> {
        vec![&self.raygen_shader, &self.hit_shader, &self.miss_shader]
    }
}

impl VulkanAsset for RaytracingPipeline {
    type ExtractedAsset = (Shader, Shader, Shader);
    type PreparedAsset = VkRaytracingPipeline;
    type Param = SRes<Assets<Shader>>;

    fn extract_asset(
        &self,
        shaders: &mut bevy::ecs::system::SystemParamItem<Self::Param>,
    ) -> Option<Self::ExtractedAsset> {
        let raygen_shader = shaders.get(&self.raygen_shader)?;
        let hit_shader = shaders.get(&self.hit_shader)?;
        let miss_shader = shaders.get(&self.miss_shader)?;
        Some((raygen_shader.clone(), hit_shader.clone(), miss_shader.clone()))
    }

    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        let (raygen_shader, hit_shader, miss_shader) = asset;
        println!("creating RT pipeline");
        let (descriptor_set_layout, pipeline_layout, vk_pipeline, nr_shader_groups) =
            create_raytracing_pipeline(&device, &raygen_shader, &hit_shader, &miss_shader);
        let shader_binding_table = create_shader_binding_table(&device, vk_pipeline, nr_shader_groups);

        let descriptor_set_alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(device.descriptor_pool)
            .set_layouts(std::slice::from_ref(&descriptor_set_layout))
            .build();

        let descriptor_set = unsafe {
            device
                .device
                .allocate_descriptor_sets(&descriptor_set_alloc_info)
                .unwrap()
        }[0];

        VkRaytracingPipeline {
            vk_pipeline,
            pipeline_layout,
            descriptor_set_layout,
            descriptor_set,
            shader_binding_table,
        }
    }

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &VkCleanup) {
        cleanup.send(VkCleanupEvent::Pipeline(asset.vk_pipeline));
        cleanup.send(VkCleanupEvent::PipelineLayout(asset.pipeline_layout));
        cleanup.send(VkCleanupEvent::DescriptorSetLayout(asset.descriptor_set_layout));
        cleanup.send(VkCleanupEvent::Buffer(asset.shader_binding_table.data.handle));
    }
}

pub struct VkRaytracingPipeline {
    pub vk_pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
    pub shader_binding_table: SBT,
}

struct ShaderGroupSizes {
    nr_raygen: u32,
    nr_miss: u32,
    nr_hit: u32,
}

impl ShaderGroupSizes {
    pub fn sum(&self) -> u32 {
        self.nr_raygen + self.nr_miss + self.nr_hit
    }
}

pub struct SBT {
    group_sizes: ShaderGroupSizes,
    pub raygen_region: vk::StridedDeviceAddressRegionKHR,
    pub miss_region: vk::StridedDeviceAddressRegionKHR,
    pub hit_region: vk::StridedDeviceAddressRegionKHR,
    pub data: Buffer<u8>,
}

pub struct RaytracingPlugin;

impl Plugin for RaytracingPlugin {
    fn build(&self, app: &mut App) {
        app.add_composed_asset::<RaytracingPipeline>();
        app.add_vulkan_asset::<RaytracingPipeline>();
    }
}

fn create_raytracing_pipeline(
    device: &RenderDevice,
    raygen_shader: &Shader,
    hit_shader: &Shader,
    miss_shader: &Shader,
) -> (
    vk::DescriptorSetLayout,
    vk::PipelineLayout,
    vk::Pipeline,
    ShaderGroupSizes,
) {
    let bindings = [
        vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
            .build(),
        vk::DescriptorSetLayoutBinding::builder()
            .binding(1)
            .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
            .build(),
    ];

    let descriptor_set_layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

    let descriptor_set_layout = unsafe {
        device
            .device
            .create_descriptor_set_layout(&descriptor_set_layout_info, None)
            .unwrap()
    };

    let push_constant_info = vk::PushConstantRange::builder()
        .stage_flags(
            vk::ShaderStageFlags::RAYGEN_KHR | vk::ShaderStageFlags::CLOSEST_HIT_KHR | vk::ShaderStageFlags::MISS_KHR,
        )
        .offset(0)
        .size(std::mem::size_of::<RaytracerRegisters>() as u32)
        .build();
    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder()
        .set_layouts(std::slice::from_ref(&descriptor_set_layout))
        .push_constant_ranges(std::slice::from_ref(&push_constant_info));

    let pipeline_layout = unsafe {
        device
            .device
            .create_pipeline_layout(&pipeline_layout_info, None)
            .unwrap()
    };

    let mut shader_stages: Vec<vk::PipelineShaderStageCreateInfo> = Vec::new();
    let mut shader_groups: Vec<vk::RayTracingShaderGroupCreateInfoKHR> = Vec::new();

    {
        shader_stages.push(device.load_shader(raygen_shader, vk::ShaderStageFlags::RAYGEN_KHR));
        shader_groups.push(
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(shader_stages.len() as u32 - 1)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
        );
    }

    {
        shader_stages.push(device.load_shader(miss_shader, vk::ShaderStageFlags::MISS_KHR));
        shader_groups.push(
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(shader_stages.len() as u32 - 1)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
        );
    }

    {
        shader_stages.push(device.load_shader(hit_shader, vk::ShaderStageFlags::CLOSEST_HIT_KHR));
        shader_groups.push(
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(shader_stages.len() as u32 - 1)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
        );
    }

    let shader_group_sizes = ShaderGroupSizes {
        nr_raygen: 1,
        nr_miss: 1,
        nr_hit: 1,
    };

    assert_eq!(shader_group_sizes.sum(), shader_groups.len() as u32);

    let pipeline_info = vk::RayTracingPipelineCreateInfoKHR::builder()
        .stages(&shader_stages)
        .groups(&shader_groups)
        .max_pipeline_ray_recursion_depth(1)
        .layout(pipeline_layout);

    let pipeline = unsafe {
        device
            .exts
            .rt_pipeline
            .create_ray_tracing_pipelines(
                vk::DeferredOperationKHR::null(),
                vk::PipelineCache::null(),
                std::slice::from_ref(&pipeline_info),
                None,
            )
            .unwrap()[0]
    };

    for stage in shader_stages {
        unsafe {
            device.device.destroy_shader_module(stage.module, None);
        }
    }

    (descriptor_set_layout, pipeline_layout, pipeline, shader_group_sizes)
}

fn create_shader_binding_table(device: &RenderDevice, pipeline: vk::Pipeline, group_sizes: ShaderGroupSizes) -> SBT {
    let raytracing_properties = get_raytracing_properties(&device);

    let handle_count = group_sizes.sum();
    let handle_size = raytracing_properties.shader_group_handle_size;
    let handle_size_aligned = vk_utils::aligned_size(handle_size, raytracing_properties.shader_group_handle_alignment);

    let mut raygen_region = vk::StridedDeviceAddressRegionKHR::default();
    let mut miss_region = vk::StridedDeviceAddressRegionKHR::default();
    let mut hit_region = vk::StridedDeviceAddressRegionKHR::default();

    raygen_region.stride =
        vk_utils::aligned_size(handle_size_aligned, raytracing_properties.shader_group_base_alignment) as u64;
    raygen_region.size = raygen_region.stride;

    miss_region.stride = handle_size_aligned as u64;
    miss_region.size = vk_utils::aligned_size(
        group_sizes.nr_miss * handle_size_aligned,
        raytracing_properties.shader_group_base_alignment,
    ) as u64;

    hit_region.stride = handle_size_aligned as u64;
    hit_region.size = vk_utils::aligned_size(
        group_sizes.nr_hit * handle_size_aligned,
        raytracing_properties.shader_group_base_alignment,
    ) as u64;

    let handle_data_size = handle_count * handle_size;

    let handle_data = unsafe {
        device
            .exts
            .rt_pipeline
            .get_ray_tracing_shader_group_handles(pipeline, 0, group_sizes.sum(), handle_data_size as usize)
            .unwrap()
    };

    let sbt_size = raygen_region.size + miss_region.size + hit_region.size;
    let mut data = device.create_host_buffer::<u8>(sbt_size, vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR);

    raygen_region.device_address = data.address;
    miss_region.device_address = data.address + raygen_region.size;
    hit_region.device_address = data.address + raygen_region.size + miss_region.size;

    {
        let mut data = device.map_buffer(&mut data);

        // memcpy syntax
        unsafe {
            let mut src = handle_data.as_ptr();
            let mut dst = data.as_ptr_mut();

            // raygen
            std::ptr::copy_nonoverlapping::<u8>(src, dst, handle_size as usize);

            // miss
            dst = data.as_ptr_mut().add(raygen_region.size as usize);
            src = src.add(handle_size as usize);

            for _ in 0..group_sizes.nr_miss {
                std::ptr::copy_nonoverlapping::<u8>(src, dst, handle_size as usize);
                src = src.add(handle_size as usize);
                dst = dst.add(handle_size_aligned as usize);
            }

            dst = data.as_ptr_mut().add(raygen_region.size as usize + miss_region.size as usize);

            // hit
            for _ in 0..group_sizes.nr_hit {
                std::ptr::copy_nonoverlapping::<u8>(src, dst, handle_size as usize);
                src = src.add(handle_size as usize);
                dst = dst.add(handle_size_aligned as usize);
            }
        }
    }

    SBT {
        group_sizes,
        data,
        raygen_region,
        miss_region,
        hit_region,
    }
}

fn get_raytracing_properties(device: &RenderDevice) -> vk::PhysicalDeviceRayTracingPipelinePropertiesKHR {
    let mut raytracing_properties = vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
    let mut properties2 = vk::PhysicalDeviceProperties2KHR::builder()
        .push_next(&mut raytracing_properties)
        .build();
    unsafe {
        device
            .instance
            .get_physical_device_properties2(device.physical_device, &mut properties2)
    }
    raytracing_properties
}

fn get_acceleration_structure_features(device: &RenderDevice) -> vk::PhysicalDeviceAccelerationStructureFeaturesKHR {
    let mut acceleration_structure_features = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default();
    let mut features2 = vk::PhysicalDeviceFeatures2KHR::builder()
        .push_next(&mut acceleration_structure_features)
        .build();
    unsafe {
        device
            .instance
            .get_physical_device_features2(device.physical_device, &mut features2)
    }

    acceleration_structure_features
}
