use ash::vk;

use crate::{
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice, render_image::VkImage,
};

#[repr(C)]
#[repr(align(8))]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

pub struct TriangleBLAS {
    pub vertex_buffer: Buffer<Vertex>,
    pub index_buffer: Buffer<u32>,
    pub geometry_to_index_offset: Buffer<u32>,
    pub geometry_to_texture: Buffer<u32>,
    pub textures: Vec<VkImage>,
    pub acceleration_structure: AccelerationStructure,
}

impl TriangleBLAS {
    pub fn get_reference(&self) -> vk::AccelerationStructureReferenceKHR {
        self.acceleration_structure.get_reference()
    }
}

#[derive(Default)]
pub struct AccelerationStructure {
    pub handle: vk::AccelerationStructureKHR,
    pub buffer: Buffer<u8>,
    pub address: u64,
}

impl AccelerationStructure {
    pub fn get_reference(&self) -> vk::AccelerationStructureReferenceKHR {
        vk::AccelerationStructureReferenceKHR {
            device_handle: self.address,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.handle != vk::AccelerationStructureKHR::null()
    }
}

pub fn allocate_acceleration_structure(
    device: &RenderDevice,
    ty: vk::AccelerationStructureTypeKHR,
    build_size: &vk::AccelerationStructureBuildSizesInfoKHR,
) -> AccelerationStructure {
    let buffer: Buffer<u8> = device.create_device_buffer(
        build_size.acceleration_structure_size,
        vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
    );

    let acceleration_structure = unsafe {
        device.exts.rt_acc_struct.create_acceleration_structure(
            &vk::AccelerationStructureCreateInfoKHR::builder()
                .ty(ty)
                .size(build_size.acceleration_structure_size)
                .buffer(buffer.handle),
            None,
        )
    }
    .unwrap();

    let address = unsafe {
        device.exts.rt_acc_struct.get_acceleration_structure_device_address(
            &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                .acceleration_structure(acceleration_structure)
                .build(),
        )
    };

    AccelerationStructure {
        handle: acceleration_structure,
        buffer,
        address,
    }
}
