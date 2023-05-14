use ash::vk;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bytemuck_derive::{Pod, Zeroable};

use crate::composed_asset::{ComposedAsset, ComposedAssetAppExtension};
use crate::render_device::RenderDevice;
use crate::shader::{Shader, ShaderProvider};
use crate::shader_binding_table::RTGroupHandle;
use crate::vk_utils;
use crate::vulkan_assets::{AddVulkanAsset, VulkanAsset};
use crate::vulkan_cleanup::{VkCleanup, VkCleanupEvent};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RaytracerRegisters {
    pub uniform_buffer_address: u64,
}

#[derive(TypeUuid)]
#[uuid = "a0b0c0d0-e0f0-11ea-87d0-0242ac130003"]
pub struct RaytracingPipeline {
    pub raygen_shader: Handle<Shader>,
    pub miss_shader: Handle<Shader>,
    pub triangle_hit_shader: Handle<Shader>,
    pub sphere_int_shader: Handle<Shader>,
    pub sphere_hit_shader: Handle<Shader>,
}

impl ComposedAsset for RaytracingPipeline {
    type DepType = Shader;
    fn get_deps(&self) -> Vec<&Handle<Self::DepType>> {
        vec![
            &self.raygen_shader,
            &self.triangle_hit_shader,
            &self.miss_shader,
            &self.sphere_int_shader,
            &self.sphere_hit_shader,
        ]
    }
}

impl VulkanAsset for RaytracingPipeline {
    type ExtractedAsset = (Shader, Shader, Shader, Shader, Shader);
    type PreparedAsset = VkRaytracingPipeline;
    type Param = SRes<Assets<Shader>>;

    fn extract_asset(
        &self,
        shaders: &mut bevy::ecs::system::SystemParamItem<Self::Param>,
    ) -> Option<Self::ExtractedAsset> {
        let raygen_shader = shaders.get(&self.raygen_shader)?;
        let miss_shader = shaders.get(&self.miss_shader)?;
        let triangle_hit_shader = shaders.get(&self.triangle_hit_shader)?;
        let sphere_int_shader = shaders.get(&self.sphere_int_shader)?;
        let sphere_hit_shader = shaders.get(&self.sphere_hit_shader)?;
        Some((
            raygen_shader.clone(),
            triangle_hit_shader.clone(),
            miss_shader.clone(),
            sphere_int_shader.clone(),
            sphere_hit_shader.clone(),
        ))
    }

    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        let (raygen_shader, triangle_hit_shader, miss_shader, sphere_int_shader, sphere_hit_shader) = asset;
        println!("creating RT pipeline");
        let (descriptor_set_layout, pipeline_layout, vk_pipeline) = create_raytracing_pipeline(
            &device,
            &raygen_shader,
            &triangle_hit_shader,
            &miss_shader,
            &sphere_int_shader,
            &sphere_hit_shader,
        );

        let rtprops = vk_utils::get_raytracing_properties(&device);
        let handle_size = rtprops.shader_group_handle_size;
        assert!(
            handle_size as usize == std::mem::size_of::<RTGroupHandle>(),
            "at the time we only support 128-bit handles (at time of writing all devices have this)"
        );

        let handle_count = 4;
        let handle_data_size = handle_count * handle_size;
        let handles: Vec<RTGroupHandle> = unsafe {
            device
                .exts
                .rt_pipeline
                .get_ray_tracing_shader_group_handles(vk_pipeline, 0, handle_count, handle_data_size as usize)
                .unwrap()
                .chunks(handle_size as usize)
                .map(|chunk| {
                    let mut handle = RTGroupHandle::default();
                    handle.copy_from_slice(chunk);
                    handle
                })
                .collect()
        };

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
            raygen_handle: handles[0],
            miss_handle: handles[1],
            triangle_hit_handle: handles[2],
            sphere_hit_handle: handles[3],
        }
    }

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &VkCleanup) {
        cleanup.send(VkCleanupEvent::Pipeline(asset.vk_pipeline));
        cleanup.send(VkCleanupEvent::PipelineLayout(asset.pipeline_layout));
        cleanup.send(VkCleanupEvent::DescriptorSetLayout(asset.descriptor_set_layout));
    }
}

pub struct VkRaytracingPipeline {
    pub vk_pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
    pub raygen_handle: RTGroupHandle,
    pub miss_handle: RTGroupHandle,
    pub triangle_hit_handle: RTGroupHandle,
    pub sphere_hit_handle: RTGroupHandle,
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
    triangle_hit_shader: &Shader,
    miss_shader: &Shader,
    sphere_int_shader: &Shader,
    sphere_hit_shader: &Shader,
) -> (vk::DescriptorSetLayout, vk::PipelineLayout, vk::Pipeline) {
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
        vk::DescriptorSetLayoutBinding::builder()
            .binding(2)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::MISS_KHR)
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
        .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
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
        shader_stages.push(device.load_shader(triangle_hit_shader, vk::ShaderStageFlags::CLOSEST_HIT_KHR));
        shader_groups.push(
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(shader_stages.len() as u32 - 1)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
        );

        shader_stages.push(device.load_shader(sphere_int_shader, vk::ShaderStageFlags::INTERSECTION_KHR));
        shader_stages.push(device.load_shader(sphere_hit_shader, vk::ShaderStageFlags::CLOSEST_HIT_KHR));
        shader_groups.push(
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(shader_stages.len() as u32 - 1)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(shader_stages.len() as u32 - 2)
                .build(),
        );
    }

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

    (descriptor_set_layout, pipeline_layout, pipeline)
}
