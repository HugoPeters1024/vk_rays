use ash::vk;
use bevy::asset::{AssetLoader, LoadedAsset, Asset};
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bevy::utils::{HashMap, HashSet};
use bytemuck_derive::{Pod, Zeroable};

use crate::composed_asset::{ComposedAssetAppExtension, ComposedAsset, ComposedAssetEvent};
use crate::render_buffer::{Buffer, BufferProvider};
use crate::render_device::RenderDevice;
use crate::render_plugin::{RenderSchedule, RenderSet};
use crate::shader::{Shader, ShaderProvider};
use crate::vk_utils;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RaytracerRegisters {
    pub uniform_buffer_address: u64,
}

#[derive(Default, TypeUuid)]
#[uuid = "a0b0c0d0-e0f0-11ea-87d0-0242ac130003"]
pub struct RaytracingPipeline {
    pub raygen_shader: Handle<Shader>,
    pub hit_shader: Handle<Shader>,
    pub miss_shader: Handle<Shader>,
    pub compiled: Option<VkRaytracingPipeline>,
}

impl ComposedAsset for RaytracingPipeline {
    type DepType = Shader;
    fn get_deps(&self) -> Vec<&Handle<Self::DepType>> {
        vec![
            &self.raygen_shader,
            &self.hit_shader,
            &self.miss_shader,
        ]
    }
}


#[derive(Component)]
pub struct VkRaytracingPipeline {
    pub vk_pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
    pub shader_binding_table: SBT,
}

pub struct SBT {
    pub handle_size_aligned: u64,
    pub raygen: Buffer<u8>,
    pub miss: Buffer<u8>,
    pub hit: Buffer<u8>,
}

impl SBT {
    pub fn get_sbt_raygen(&self) -> vk::StridedDeviceAddressRegionKHR {
        vk::StridedDeviceAddressRegionKHR::builder()
            .device_address(self.raygen.address)
            .stride(self.handle_size_aligned)
            .size(self.handle_size_aligned)
            .build()
    }

    pub fn get_sbt_miss(&self) -> vk::StridedDeviceAddressRegionKHR {
        vk::StridedDeviceAddressRegionKHR::builder()
            .device_address(self.miss.address)
            .stride(self.handle_size_aligned)
            .size(self.handle_size_aligned)
            .build()
    }

    pub fn get_sbt_hit(&self) -> vk::StridedDeviceAddressRegionKHR {
        vk::StridedDeviceAddressRegionKHR::builder()
            .device_address(self.hit.address)
            .stride(self.handle_size_aligned)
            .size(self.handle_size_aligned)
            .build()
    }
}


pub struct RaytracingPlugin;

impl Plugin for RaytracingPlugin {
    fn build(&self, app: &mut App) {
        app.add_composed_asset::<RaytracingPipeline>();
        app.edit_schedule(RenderSchedule, |schedule| {
            schedule.add_system(ensure_pipeline_up_to_date.in_set(RenderSet::Extract));
        });
    }
}

fn ensure_pipeline_up_to_date(
    device: Res<RenderDevice>,
    shaders: Res<Assets<Shader>>,
    mut pipeline_events: EventReader<ComposedAssetEvent<RaytracingPipeline>>,
    mut pipelines: ResMut<Assets<RaytracingPipeline>>,
) {
    for event in pipeline_events.iter() {
        let handle = match event {
            ComposedAssetEvent(AssetEvent::Created { handle }) => handle,
            ComposedAssetEvent(AssetEvent::Modified { handle }) => handle,
            ComposedAssetEvent(AssetEvent::Removed { handle: _ }) => panic!("TODO"),
        };

        let mut pipeline = pipelines.get_mut(handle).unwrap();
        let raygen_shader = shaders.get(&pipeline.raygen_shader);
        let hit_shader = shaders.get(&pipeline.hit_shader);
        let miss_shader = shaders.get(&pipeline.miss_shader);

        if raygen_shader.is_none() || hit_shader.is_none() || miss_shader.is_none() {
            println!("Not all shaders loaded, skipping pipeline reload");
            continue;
        }

        println!("creating RT pipeline");

        let (descriptor_set_layout, pipeline_layout, vk_pipeline, nr_shader_groups) =
            create_raytracing_pipeline(
                &device,
                raygen_shader.unwrap(),
                hit_shader.unwrap(),
                miss_shader.unwrap(),
            );
        let shader_binding_table =
            create_shader_binding_table(&device, vk_pipeline, nr_shader_groups as u32);

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

        pipeline.compiled = Some(VkRaytracingPipeline {
                vk_pipeline,
                pipeline_layout,
                descriptor_set_layout,
                descriptor_set,
                shader_binding_table,
            });
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
    usize,
) {
    let bindings = [
        vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
            .build(),
        //vk::DescriptorSetLayoutBinding::builder()
        //    .binding(1)
        //    .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
        //    .descriptor_count(1)
        //    .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
        //    .build(),
    ];

    let descriptor_set_layout_info =
        vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

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

    (
        descriptor_set_layout,
        pipeline_layout,
        pipeline,
        shader_groups.len(),
    )
}

fn create_shader_binding_table(
    device: &RenderDevice,
    pipeline: vk::Pipeline,
    group_count: u32,
) -> SBT {
    let raytracing_properties = get_raytracing_properties(&device);
    let handle_size = raytracing_properties.shader_group_handle_size;
    let handle_size_aligned = vk_utils::aligned_size(
        handle_size,
        raytracing_properties.shader_group_handle_alignment,
    ) as usize;
    let sbt_size = group_count as usize * handle_size_aligned;

    let handle_data = unsafe {
        device
            .exts
            .rt_pipeline
            .get_ray_tracing_shader_group_handles(pipeline, 0, group_count, sbt_size as usize)
            .unwrap()
    };

    let mut raygen = device.create_host_buffer::<u8>(
        &device,
        handle_size as u64,
        vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR,
    );
    let mut miss = device.create_host_buffer::<u8>(
        &device,
        handle_size as u64,
        vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR,
    );
    let mut hit = device.create_host_buffer::<u8>(
        &device,
        handle_size as u64,
        vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR,
    );

    for (i, b) in handle_data
        .iter()
        .skip(handle_size_aligned * 0)
        .take(handle_size as usize)
        .enumerate()
    {
        raygen[i] = *b;
    }
    for (i, b) in handle_data
        .iter()
        .skip(handle_size_aligned * 1)
        .take(handle_size as usize)
        .enumerate()
    {
        miss[i] = *b;
    }
    for (i, b) in handle_data
        .iter()
        .skip(handle_size_aligned * 2)
        .take(handle_size as usize)
        .enumerate()
    {
        hit[i] = *b;
    }

    SBT {
        handle_size_aligned: handle_size_aligned as u64,
        raygen,
        miss,
        hit,
    }
}

fn get_raytracing_properties(
    device: &RenderDevice,
) -> vk::PhysicalDeviceRayTracingPipelinePropertiesKHR {
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

fn get_acceleration_structure_features(
    device: &RenderDevice,
) -> vk::PhysicalDeviceAccelerationStructureFeaturesKHR {
    let mut acceleration_structure_features =
        vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default();
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
