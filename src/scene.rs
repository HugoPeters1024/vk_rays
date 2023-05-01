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

#[derive(Resource)]
pub enum Scene {
    Empty,
    Ready(AccelerationStructure),
}

impl Default for Scene {
    fn default() -> Self {
        Scene::Empty
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
    mut should_rebuild: Local<bool>,
    mut commands: Commands,
    device: Res<RenderDevice>,
    meshes: Query<Ref<Handle<GltfMesh>>>,
    blasses: Res<VulkanAssets<GltfMesh>>,
) {
    if meshes.iter().any(|m| m.is_changed()) {
        *should_rebuild = true;
    }

    if !*should_rebuild {
        return;
    }
    println!("REBUILDING SCENE");

    let mut resolved_blasses: Vec<&BLAS> = Vec::new();
    for mesh in meshes.iter() {
        let Some(blas) = blasses.get(&mesh) else {
            println!("No BLAS for mesh");
            return;
        };
        resolved_blasses.push(blas);
    }

    *should_rebuild = false;

    let instances = resolved_blasses
        .iter()
        .map(|blas| {
            let transform = vk::TransformMatrixKHR {
                matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            };

            vk::AccelerationStructureInstanceKHR {
                transform,
                instance_custom_index_and_mask: Packed24_8::new(0, 0xFF),
                instance_shader_binding_table_record_offset_and_flags: Packed24_8::new(
                    0, 0b1, //vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE,
                ),
                acceleration_structure_reference: blas.get_reference(),
            }
        })
        .collect::<Vec<_>>();

    let mut instance_buffer = device.create_host_buffer::<vk::AccelerationStructureInstanceKHR>(
        instances.len() as u64,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
    );

    let mut instance_buffer_view = device.map_buffer(&mut instance_buffer);
    for (i, instance) in instances.iter().enumerate() {
        instance_buffer_view[i] = instance.clone();
    }
    drop(instance_buffer_view);

    let geometry = vk::AccelerationStructureGeometryKHR::builder()
        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
        .flags(vk::GeometryFlagsKHR::OPAQUE)
        .geometry(vk::AccelerationStructureGeometryDataKHR {
            instances: vk::AccelerationStructureGeometryInstancesDataKHR::builder()
                .array_of_pointers(false)
                .data(vk::DeviceOrHostAddressConstKHR {
                    device_address: instance_buffer.address,
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
        device
            .exts
            .rt_acc_struct
            .get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &build_geometry,
                std::slice::from_ref(&primitive_count),
            )
    };

    let acceleration_structure_buffer = device.create_device_buffer(
        build_sizes.acceleration_structure_size,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
    );

    let acceleration_structure_info = vk::AccelerationStructureCreateInfoKHR::builder()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .buffer(acceleration_structure_buffer.handle)
        .size(build_sizes.acceleration_structure_size)
        .build();

    let acceleration_structure = unsafe {
        device
            .exts
            .rt_acc_struct
            .create_acceleration_structure(&acceleration_structure_info, None)
    }
    .unwrap();

    let scratch_buffer: Buffer<u8> = device.create_device_buffer(
        build_sizes.build_scratch_size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    );

    let build_geometry = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
        .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
        .dst_acceleration_structure(acceleration_structure)
        .geometries(std::slice::from_ref(&geometry))
        .scratch_data(vk::DeviceOrHostAddressKHR {
            device_address: scratch_buffer.address,
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

    let address = unsafe {
        device
            .exts
            .rt_acc_struct
            .get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                    .acceleration_structure(acceleration_structure)
                    .build(),
            )
    };

    cleanup.send(VkCleanupEvent::Buffer(scratch_buffer.handle));
    cleanup.send(VkCleanupEvent::Buffer(instance_buffer.handle));

    commands.insert_resource(Scene::Ready(AccelerationStructure {
        handle: acceleration_structure,
        buffer: acceleration_structure_buffer,
        address,
    }));
}

fn destroy_scene(scene: Res<Scene>, cleanup: Res<VkCleanup>) {
    if let Scene::Ready(tlas) = scene.into_inner() {
        cleanup.send(VkCleanupEvent::AccelerationStructure(tlas.handle));
        cleanup.send(VkCleanupEvent::Buffer(tlas.buffer.handle));
    }
}
