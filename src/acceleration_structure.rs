use ash::vk;

use crate::{
    gltf_assets::GltfMesh,
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice,
    vulkan_assets::VulkanAsset,
    vulkan_cleanup::{VkCleanup, VkCleanupEvent},
};

#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 3],
}

pub struct BLAS {
    pub vertex_buffer: Buffer<Vertex>,
    pub index_buffer: Buffer<u32>,
    pub acceleration_structure: AccelerationStructure,
}

impl BLAS {
    pub fn get_reference(&self) -> vk::AccelerationStructureReferenceKHR {
        self.acceleration_structure.get_reference()
    }
}

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
}

struct GeometryDescr {
    first_vertex: usize,
    vertex_count: usize,
    first_index: usize,
    index_count: usize,
}

impl VulkanAsset for GltfMesh {
    type ExtractedAsset = GltfMesh;
    type PreparedAsset = BLAS;
    type Param = ();

    fn extract_asset(
        &self,
        _param: &mut bevy::ecs::system::SystemParamItem<Self::Param>,
    ) -> Option<Self::ExtractedAsset> {
        Some(self.clone())
    }

    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        let mesh = asset.single_mesh();
        let (vertex_count, index_count) = extract_mesh_sizes(&mesh);

        let mut vertex_buffer_host: Buffer<Vertex> = device.create_host_buffer(
            vertex_count as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
        );
        let mut index_buffer_host: Buffer<u32> = device.create_host_buffer(
            index_count as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
        );

        let mut vertex_buffer_view = device.map_buffer(&mut vertex_buffer_host);
        let mut index_buffer_view = device.map_buffer(&mut index_buffer_host);

        let geometries_descrs = extract_mesh_data(
            &asset,
            vertex_buffer_view.as_slice_mut(),
            index_buffer_view.as_slice_mut(),
        );

        drop(index_buffer_view);
        drop(vertex_buffer_view);

        println!(
            "Building BLAS with {} vertices and {} indices, divided over {} geometries",
            vertex_count,
            index_count,
            geometries_descrs.len()
        );
        println!("Uploading data to GPU");

        let vertex_buffer_device: Buffer<Vertex> = device.create_device_buffer(
            vertex_count as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
        );

        let index_buffer_device: Buffer<u32> = device.create_device_buffer(
            index_count as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
        );

        device.run_asset_commands(|cmd_buffer| {
            device.upload_buffer(cmd_buffer, &mut vertex_buffer_host, &vertex_buffer_device);
            device.upload_buffer(cmd_buffer, &mut index_buffer_host, &index_buffer_device);
        });

        device.destroy_buffer(vertex_buffer_host);
        device.destroy_buffer(index_buffer_host);

        let geometry_infos = geometries_descrs
            .iter()
            .map(|geometry| {
                vk::AccelerationStructureGeometryKHR::builder()
                    .flags(vk::GeometryFlagsKHR::OPAQUE)
                    .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
                    .geometry(vk::AccelerationStructureGeometryDataKHR {
                        triangles: vk::AccelerationStructureGeometryTrianglesDataKHR::builder()
                            .vertex_format(vk::Format::R32G32B32_SFLOAT)
                            .vertex_data(vk::DeviceOrHostAddressConstKHR {
                                device_address: vertex_buffer_device.address,
                            })
                            .vertex_stride(std::mem::size_of::<Vertex>() as u64)
                            .max_vertex(geometry.vertex_count as u32)
                            .index_type(vk::IndexType::UINT32)
                            .index_data(vk::DeviceOrHostAddressConstKHR {
                                device_address: index_buffer_device.address,
                            })
                            .transform_data(vk::DeviceOrHostAddressConstKHR { device_address: 0 })
                            .build(),
                    })
                    .build()
            })
            .collect::<Vec<_>>();

        let combined_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&geometry_infos);

        let primitive_counts = geometries_descrs
            .iter()
            .map(|geometry| (geometry.index_count / 3) as u32)
            .collect::<Vec<_>>();

        let geometry_sizes = unsafe {
            device
                .exts
                .rt_acc_struct
                .get_acceleration_structure_build_sizes(
                    vk::AccelerationStructureBuildTypeKHR::DEVICE,
                    &combined_build_info,
                    &primitive_counts,
                )
        };

        let num_triangles = primitive_counts.iter().sum::<u32>();

        let mut acceleration_structure = allocate_acceleration_structure(
            &device,
            vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            &geometry_sizes,
        );

        let scratch_buffer: Buffer<u8> = device.create_device_buffer(
            geometry_sizes.build_scratch_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        );

        let build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .dst_acceleration_structure(acceleration_structure.handle)
            .geometries(&geometry_infos)
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: scratch_buffer.address,
            })
            .build();

        let build_range_info = vk::AccelerationStructureBuildRangeInfoKHR::builder()
            .primitive_count(num_triangles)
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
            device
                .exts
                .rt_acc_struct
                .get_acceleration_structure_device_address(
                    &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                        .acceleration_structure(acceleration_structure.handle)
                        .build(),
                )
        };

        let blas = BLAS {
            vertex_buffer: vertex_buffer_device,
            index_buffer: index_buffer_device,
            acceleration_structure,
        };

        blas
    }

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &VkCleanup) {
        cleanup.send(VkCleanupEvent::Buffer(asset.vertex_buffer.handle));
        cleanup.send(VkCleanupEvent::Buffer(asset.index_buffer.handle));
        cleanup.send(VkCleanupEvent::AccelerationStructure(
            asset.acceleration_structure.handle,
        ));
        cleanup.send(VkCleanupEvent::Buffer(
            asset.acceleration_structure.buffer.handle,
        ));
    }
}

fn extract_mesh_sizes(mesh: &gltf::Mesh) -> (usize, usize) {
    let mut vertex_count = 0;
    let mut index_count = 0;
    for primitive in mesh.primitives() {
        let positions = primitive
            .attributes()
            .find_map(|(s, a)| {
                if s == gltf::Semantic::Positions {
                    Some(a)
                } else {
                    None
                }
            })
            .unwrap();
        vertex_count += positions.count();

        index_count += primitive.indices().unwrap().count();
    }
    (vertex_count, index_count)
}

fn extract_mesh_data(
    gltf: &GltfMesh,
    vertex_buffer: &mut [Vertex],
    index_buffer: &mut [u32],
) -> Vec<GeometryDescr> {
    let mesh = gltf.single_mesh();
    let mut geometries = Vec::new();
    let mut vertex_buffer_head = 0;
    let mut index_buffer_head = 0;
    for primitive in mesh.primitives() {
        let positions = primitive
            .attributes()
            .find_map(|(s, a)| {
                if s == gltf::Semantic::Positions {
                    Some(a)
                } else {
                    None
                }
            })
            .unwrap();
        let indices = primitive.indices().unwrap();

        let geometry = GeometryDescr {
            first_vertex: vertex_buffer_head,
            vertex_count: positions.count(),
            first_index: index_buffer_head,
            index_count: indices.count(),
        };

        let reader = primitive.reader(|buffer| Some(&gltf.buffers[buffer.index()]));
        let pos_reader = reader.read_positions().unwrap();

        assert!(pos_reader.len() == geometry.vertex_count);

        for (i, pos) in pos_reader.enumerate() {
            vertex_buffer[geometry.first_vertex + i] = Vertex { pos };
        }

        let index_reader = reader.read_indices().unwrap().into_u32();
        assert!(index_reader.len() == geometry.index_count);
        assert!(geometry.index_count % 3 == 0);

        for (i, index) in index_reader.enumerate() {
            index_buffer[geometry.first_index + i] = index;
        }

        vertex_buffer_head += geometry.vertex_count;
        index_buffer_head += geometry.index_count;
        geometries.push(geometry);
    }

    geometries
}

fn allocate_acceleration_structure(
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
        device
            .exts
            .rt_acc_struct
            .get_acceleration_structure_device_address(
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
