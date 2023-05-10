use ash::vk;
use bevy::prelude::*;

use crate::{
    acceleration_structure::{allocate_acceleration_structure, AccelerationStructure},
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice,
};

#[derive(Component, Default)]
pub struct Sphere;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AABB {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
}

impl Default for AABB {
    fn default() -> Self {
        Self {
            min_x: -0.5,
            min_y: -0.5,
            min_z: -0.5,
            max_x: 0.5,
            max_y: 0.5,
            max_z: 0.5,
        }
    }
}

impl AABB {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self {
            min_x: min.x,
            min_y: min.y,
            min_z: min.z,
            max_x: max.x,
            max_y: max.y,
            max_z: max.z,
        }
    }

    pub fn min(&self) -> Vec3 {
        Vec3::new(self.min_x, self.min_y, self.min_z)
    }

    pub fn max(&self) -> Vec3 {
        Vec3::new(self.max_x, self.max_y, self.max_z)
    }
}

#[derive(Resource)]
pub struct SphereBLAS {
    pub sphere_buffer: Buffer<AABB>,
    pub acceleration_structure: AccelerationStructure,
}

impl SphereBLAS {
    pub fn get_reference(&self) -> vk::AccelerationStructureReferenceKHR {
        self.acceleration_structure.get_reference()
    }

    pub fn make_one(aabb: &AABB, device: &RenderDevice) -> Self {
        let mut aabb_buffer_host: Buffer<AABB> = device.create_host_buffer(
            1,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
        );

        {
            let mut aabb_buffer = device.map_buffer(&mut aabb_buffer_host);
            aabb_buffer[0] = aabb.clone();
            dbg!(&aabb_buffer[0]);
        }

        let aabb_buffer_device: Buffer<AABB> = device.create_device_buffer(
            1,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
        );
        device.run_asset_commands(|cmd_buffer| {
            device.upload_buffer(cmd_buffer, &mut aabb_buffer_host, &aabb_buffer_device);
        });

        device.destroy_buffer(aabb_buffer_host);

        let geometry_info = vk::AccelerationStructureGeometryKHR::builder()
            .flags(vk::GeometryFlagsKHR::OPAQUE)
            .geometry_type(vk::GeometryTypeKHR::AABBS)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                aabbs: vk::AccelerationStructureGeometryAabbsDataKHR::builder()
                    .stride(std::mem::size_of::<AABB>() as u64)
                    .data(vk::DeviceOrHostAddressConstKHR {
                        device_address: aabb_buffer_device.address,
                    })
                    .build(),
            });

        let combined_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(std::slice::from_ref(&geometry_info));

        let primitive_counts = [1];

        let geometry_sizes = unsafe {
            device.exts.rt_acc_struct.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &combined_build_info,
                &primitive_counts,
            )
        };

        let mut acceleration_structure =
            allocate_acceleration_structure(device, vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL, &geometry_sizes);

        let scratch_buffer: Buffer<u8> =
            device.create_device_buffer(geometry_sizes.build_scratch_size, vk::BufferUsageFlags::STORAGE_BUFFER);

        let build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .dst_acceleration_structure(acceleration_structure.handle)
            .geometries(std::slice::from_ref(&geometry_info))
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: scratch_buffer.address,
            })
            .build();

        let build_range_info = vk::AccelerationStructureBuildRangeInfoKHR::builder()
            .primitive_count(1)
            // offset in bytes where the primitive data is defined
            .primitive_offset(0)
            .first_vertex(0)
            .transform_offset(0)
            .build();

        let build_range_infos = std::slice::from_ref(&build_range_info);

        unsafe {
            device.run_asset_commands(&|cmd_buffer| {
                device.exts.rt_acc_struct.cmd_build_acceleration_structures(
                    cmd_buffer,
                    std::slice::from_ref(&build_geometry_info),
                    std::slice::from_ref(&build_range_infos),
                );
            })
        }

        device.destroy_buffer(scratch_buffer);

        acceleration_structure.address = unsafe {
            device.exts.rt_acc_struct.get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                    .acceleration_structure(acceleration_structure.handle)
                    .build(),
            )
        };

        Self {
            sphere_buffer: aabb_buffer_device,
            acceleration_structure,
        }
    }
}
