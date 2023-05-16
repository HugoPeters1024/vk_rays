use ash::vk;
use bevy::{prelude::*, utils::HashMap, asset::HandleId};

use crate::{
    gltf_assets::GltfMesh,
    raytracing_pipeline::RaytracingPipeline,
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice,
    render_plugin::{RenderSchedule, RenderSet},
    vk_utils,
    vulkan_assets::{VulkanAssets, VkAssetCleanupPlaybook},
    vulkan_cleanup::{VkCleanup, VkCleanupEvent},
};

pub type RTGroupHandle = [u8; 32];

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SBTRegionRaygen {
    pub handle: RTGroupHandle,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SBTRegionMiss {
    pub handle: RTGroupHandle,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub enum SBTRegionHitEntry {
    Triangle(SBTRegionHitTriangle),
    Sphere(SBTRegionHitSphere),
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SBTRegionHitTriangle {
    pub handle: RTGroupHandle,
    pub vertex_buffer: u64,
    pub index_buffer: u64,
    pub geometry_to_index_offset_buffer: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SBTRegionHitSphere {
    pub handle: RTGroupHandle,
}

#[derive(Resource, Default)]
pub struct SBT {
    pub raygen_region: vk::StridedDeviceAddressRegionKHR,
    pub miss_region: vk::StridedDeviceAddressRegionKHR,
    pub hit_region: vk::StridedDeviceAddressRegionKHR,
    pub data: Buffer<u8>,
    pub triangle_offsets: HashMap<HandleId, u32>
}

pub struct SBTPlugin;

impl Plugin for SBTPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SBT>();
        app.edit_schedule(RenderSchedule, |schedule| {
            schedule.add_system(
                update_sbt
                    .run_if(
                        resource_changed::<VulkanAssets<GltfMesh>>()
                            .or_else(resource_changed::<VulkanAssets<RaytracingPipeline>>()),
                    )
                    .in_set(RenderSet::Extract),
            );
        });

        let mut cleanup_schedule = app.world.get_resource_mut::<VkAssetCleanupPlaybook>().unwrap();
        cleanup_schedule.add_system(destroy_sbt);
    }
}

fn update_sbt(
    mut me: ResMut<SBT>,
    device: Res<RenderDevice>,
    cleanup: Res<VkCleanup>,
    pipeline: Res<VulkanAssets<RaytracingPipeline>>,
    triangle_meshes: Res<VulkanAssets<GltfMesh>>,
) {
    let Some(pipeline) = pipeline.get_single() else {
        println!("Bailing, No pipeline");
        return;
    };
    println!("Updating SBT");
    let rtprops = vk_utils::get_raytracing_properties(&device);

    let raygen_region_data = SBTRegionRaygen {
        handle: pipeline.raygen_handle,
    };
    let miss_region_data = SBTRegionMiss {
        handle: pipeline.miss_handle,
    };

    let mut hit_region_data = Vec::new();
    hit_region_data.push(SBTRegionHitEntry::Sphere(SBTRegionHitSphere {
        handle: pipeline.sphere_hit_handle,
    }));

    me.triangle_offsets.clear();
    for (handle, mesh) in triangle_meshes.items() {
        hit_region_data.push(SBTRegionHitEntry::Triangle(SBTRegionHitTriangle {
            handle: pipeline.triangle_hit_handle,
            vertex_buffer: mesh.vertex_buffer.address,
            index_buffer: mesh.index_buffer.address,
            geometry_to_index_offset_buffer: mesh.geometry_to_index_offset.address,
        }));
        me.triangle_offsets.insert(handle.clone(), hit_region_data.len() as u32 - 1);
    }

    let handle_size_aligned = vk_utils::aligned_size(
        std::mem::size_of::<RTGroupHandle>() as u32,
        rtprops.shader_group_handle_alignment,
    );

    me.raygen_region.stride = vk_utils::aligned_size(handle_size_aligned, rtprops.shader_group_base_alignment) as u64;
    me.raygen_region.size = me.raygen_region.stride;

    me.miss_region.stride = handle_size_aligned as u64;
    me.miss_region.size =
        vk_utils::aligned_size(me.miss_region.stride as u32, rtprops.shader_group_base_alignment) as u64;

    let hit_entry_size = vk_utils::aligned_size(
        [
            std::mem::size_of::<SBTRegionHitTriangle>(),
            std::mem::size_of::<SBTRegionHitSphere>(),
        ]
        .into_iter()
        .max()
        .unwrap() as u32,
        rtprops.shader_group_base_alignment,
    );
    me.hit_region.stride = hit_entry_size as u64;
    me.hit_region.size = vk_utils::aligned_size(
        hit_region_data.len() as u32 * me.hit_region.stride as u32,
        rtprops.shader_group_base_alignment,
    ) as u64;

    let sbt_size = me.raygen_region.size + me.miss_region.size + me.hit_region.size;

    if me.data.nr_elements != sbt_size {
        cleanup.send(VkCleanupEvent::Buffer(me.data.handle));
        me.data = device.create_host_buffer::<u8>(sbt_size, vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR);
    }

    me.raygen_region.device_address = me.data.address;
    me.miss_region.device_address = me.data.address + me.raygen_region.size;
    me.hit_region.device_address = me.data.address + me.raygen_region.size + me.miss_region.size;

    {
        let mut data = device.map_buffer(&mut me.data);
        unsafe {
            let mut dst = data.as_ptr_mut();

            // raygen region (only a handle)
            (dst as *mut SBTRegionRaygen).write(raygen_region_data);
            dst = dst.add(me.raygen_region.size as usize);

            // miss region (comes after the raygen region)
            (dst as *mut SBTRegionMiss).write(miss_region_data);
            dst = dst.add(me.miss_region.size as usize);

            for hit_entry in hit_region_data.iter() {
                match hit_entry {
                    SBTRegionHitEntry::Triangle(data) => {
                        (dst as *mut SBTRegionHitTriangle).write(*data);
                    }
                    SBTRegionHitEntry::Sphere(data) => {
                        (dst as *mut SBTRegionHitSphere).write(*data);
                    }
                }
                dst = dst.add(me.hit_region.stride as usize);
            }
        }
    }
}

fn destroy_sbt(me: Res<SBT>, cleanup: Res<VkCleanup>) {
    cleanup.send(VkCleanupEvent::Buffer(me.data.handle));
}
