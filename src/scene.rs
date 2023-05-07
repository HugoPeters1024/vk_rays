use ash::vk::{self, Packed24_8};
use bevy::prelude::*;

use crate::{
    acceleration_structure::{AccelerationStructure, BLAS},
    gltf_assets::GltfMesh,
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice,
    vulkan_assets::{VkAssetCleanupPlaybook, VulkanAssets},
    vulkan_cleanup::{VkCleanup, VkCleanupEvent},
};

#[derive(Resource, Default)]
pub struct Scene {
    pub tlas: AccelerationStructure,
    scratch_buffer: Buffer<u8>,
    instance_buffer: Buffer<vk::AccelerationStructureInstanceKHR>,
}

impl Scene {
    pub fn is_ready(&self) -> bool {
        self.tlas.is_ready()
    }
}

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.world.init_resource::<Scene>();
        app.add_system(update_scene);

        app.world
            .get_resource_mut::<VkAssetCleanupPlaybook>()
            .unwrap()
            .add_system(destroy_scene);
    }
}

fn update_scene(
    cleanup: Res<VkCleanup>,
    mut scene: ResMut<Scene>,
    device: Res<RenderDevice>,
    meshes: Query<(&GlobalTransform, &Handle<GltfMesh>)>,
    blasses: Res<VulkanAssets<GltfMesh>>,
) {
    let mut resolved_blasses: Vec<(&GlobalTransform, &BLAS)> = Vec::new();
    for (t, mesh) in meshes.iter() {
        let Some(blas) = blasses.get(&mesh) else {
            continue;
        };
        resolved_blasses.push((t, blas));
    }

    let instances = resolved_blasses
        .into_iter()
        .enumerate()
        .map(|(i, (transform, blas))| {
            let columns = transform.affine().to_cols_array_2d();
            let transform = vk::TransformMatrixKHR {
                matrix: [
                    columns[0][0],
                    columns[1][0],
                    columns[2][0],
                    columns[3][0],
                    columns[0][1],
                    columns[1][1],
                    columns[2][1],
                    columns[3][1],
                    columns[0][2],
                    columns[1][2],
                    columns[2][2],
                    columns[3][2],
                ],
            };

            vk::AccelerationStructureInstanceKHR {
                transform,
                instance_custom_index_and_mask: Packed24_8::new(i as u32, 0xFF),
                instance_shader_binding_table_record_offset_and_flags: Packed24_8::new(
                    0, 0b1, //vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE,
                ),
                acceleration_structure_reference: blas.get_reference(),
            }
        })
        .collect::<Vec<_>>();

    if instances.is_empty() {
        return;
    }

    if instances.len() != scene.instance_buffer.nr_elements as usize {
        println!("Scene: Resizing instance buffer to {} elements", instances.len());
        cleanup.send(VkCleanupEvent::Buffer(scene.instance_buffer.handle));
        scene.instance_buffer = device.create_host_buffer::<vk::AccelerationStructureInstanceKHR>(
            instances.len() as u64,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
        );
    }

    let mut instance_buffer_view = device.map_buffer(&mut scene.instance_buffer);
    for (i, instance) in instances.iter().enumerate() {
        instance_buffer_view[i] = instance.clone();
    }
    drop(instance_buffer_view);

    // we always rebuild the tlas, better to destroy it before the underlying buffer
    cleanup.send(VkCleanupEvent::AccelerationStructure(scene.tlas.handle));

    let geometry = vk::AccelerationStructureGeometryKHR::builder()
        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
        .flags(vk::GeometryFlagsKHR::OPAQUE)
        .geometry(vk::AccelerationStructureGeometryDataKHR {
            instances: vk::AccelerationStructureGeometryInstancesDataKHR::builder()
                .array_of_pointers(false)
                .data(vk::DeviceOrHostAddressConstKHR {
                    device_address: scene.instance_buffer.address,
                })
                .build(),
        })
        .build();

    let build_geometry = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
        .geometries(std::slice::from_ref(&geometry))
        .build();

    let primitive_count = instances.len() as u32;

    let build_sizes = unsafe {
        device.exts.rt_acc_struct.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            &build_geometry,
            std::slice::from_ref(&primitive_count),
        )
    };

    if build_sizes.acceleration_structure_size != scene.tlas.buffer.nr_elements {
        println!("Scene: Resizing TLAS to {} bytes", build_sizes.acceleration_structure_size);
        cleanup.send(VkCleanupEvent::Buffer(scene.tlas.buffer.handle));
        scene.tlas.buffer = device.create_device_buffer(
            build_sizes.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
        );
    }

    let acceleration_structure_info = vk::AccelerationStructureCreateInfoKHR::builder()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .buffer(scene.tlas.buffer.handle)
        .size(build_sizes.acceleration_structure_size)
        .build();

    scene.tlas.handle = unsafe {
        device
            .exts
            .rt_acc_struct
            .create_acceleration_structure(&acceleration_structure_info, None)
    }
    .unwrap();

    if build_sizes.build_scratch_size != scene.scratch_buffer.nr_elements {
        println!("Scene: Resizing scratch buffer to {} bytes", build_sizes.build_scratch_size);
        cleanup.send(VkCleanupEvent::Buffer(scene.scratch_buffer.handle));
        scene.scratch_buffer =
            device.create_device_buffer(build_sizes.build_scratch_size, vk::BufferUsageFlags::STORAGE_BUFFER);
    }

    let build_geometry = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
        .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
        .dst_acceleration_structure(scene.tlas.handle)
        .geometries(std::slice::from_ref(&geometry))
        .scratch_data(vk::DeviceOrHostAddressKHR {
            device_address: scene.scratch_buffer.address,
        });

    let build_range = vk::AccelerationStructureBuildRangeInfoKHR::builder()
        .primitive_count(primitive_count)
        .primitive_offset(0)
        .first_vertex(0)
        .transform_offset(0)
        .build();

    let build_range_infos = std::slice::from_ref(&build_range);
    unsafe {
        device.run_single_commands(&|command_buffer| {
            device.exts.rt_acc_struct.cmd_build_acceleration_structures(
                command_buffer,
                std::slice::from_ref(&build_geometry),
                std::slice::from_ref(&build_range_infos),
            );
        });
    }

    scene.tlas.address = unsafe {
        device.exts.rt_acc_struct.get_acceleration_structure_device_address(
            &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                .acceleration_structure(scene.tlas.handle)
                .build(),
        )
    };
}

fn destroy_scene(scene: Res<Scene>, cleanup: Res<VkCleanup>) {
    cleanup.send(VkCleanupEvent::Buffer(scene.tlas.buffer.handle));
    cleanup.send(VkCleanupEvent::AccelerationStructure(scene.tlas.handle));
    cleanup.send(VkCleanupEvent::Buffer(scene.instance_buffer.handle));
    cleanup.send(VkCleanupEvent::Buffer(scene.scratch_buffer.handle));
}
