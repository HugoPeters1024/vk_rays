use ash::vk;
use bevy::{
    asset::{AssetLoader, LoadedAsset},
    reflect::TypeUuid,
};

use crate::{
    acceleration_structure::{allocate_acceleration_structure, Vertex, TriangleBLAS},
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice,
    vulkan_assets::VulkanAsset,
    vulkan_cleanup::{VkCleanup, VkCleanupEvent},
};

#[derive(TypeUuid, Default, Clone)]
#[uuid = "ddd211b2-ba53-47d2-a40b-15fd29d757c6"]
pub struct GltfMesh {
    pub document: Option<gltf::Document>,
    pub buffers: Vec<gltf::buffer::Data>,
    pub images: Vec<gltf::image::Data>,
}

impl GltfMesh {
    pub fn single_mesh(&self) -> gltf::Mesh {
        let document = self.document.as_ref().unwrap();
        let scene = document.default_scene().unwrap();
        let mut node = scene.nodes().next().unwrap();
        while node.mesh().is_none() {
            node = node.children().next().unwrap();
        }

        return node.mesh().unwrap();
    }
}

#[derive(Default)]
pub struct GltfLoader;

impl AssetLoader for GltfLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut bevy::asset::LoadContext,
    ) -> bevy::utils::BoxedFuture<'a, Result<(), bevy::asset::Error>> {
        Box::pin(async move {
            let (document, buffers, images) = gltf::import_slice(bytes)?;

            let asset = GltfMesh {
                document: Some(document),
                buffers,
                images,
            };

            println!(
                "GLTF {} has {} chunks of buffer data",
                load_context.path().display(),
                asset.buffers.len()
            );
            println!(
                "GLTF {} has {} chunks of image data",
                load_context.path().display(),
                asset.images.len()
            );

            load_context.set_default_asset(LoadedAsset::new(asset));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["gltf", "glb"]
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
    type PreparedAsset = TriangleBLAS;
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
            device.exts.rt_acc_struct.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &combined_build_info,
                &primitive_counts,
            )
        };

        let num_triangles = primitive_counts.iter().sum::<u32>();

        let mut acceleration_structure =
            allocate_acceleration_structure(&device, vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL, &geometry_sizes);

        let scratch_buffer: Buffer<u8> =
            device.create_device_buffer(geometry_sizes.build_scratch_size, vk::BufferUsageFlags::STORAGE_BUFFER);

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
            device.exts.rt_acc_struct.get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                    .acceleration_structure(acceleration_structure.handle)
                    .build(),
            )
        };

        let blas = TriangleBLAS {
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
        cleanup.send(VkCleanupEvent::Buffer(asset.acceleration_structure.buffer.handle));
    }
}

fn extract_mesh_sizes(mesh: &gltf::Mesh) -> (usize, usize) {
    let mut vertex_count = 0;
    let mut index_count = 0;
    for primitive in mesh.primitives() {
        let positions = primitive
            .attributes()
            .find_map(|(s, a)| if s == gltf::Semantic::Positions { Some(a) } else { None })
            .unwrap();
        vertex_count += positions.count();

        index_count += primitive.indices().unwrap().count();
    }
    (vertex_count, index_count)
}

fn extract_mesh_data(gltf: &GltfMesh, vertex_buffer: &mut [Vertex], index_buffer: &mut [u32]) -> Vec<GeometryDescr> {
    let mesh = gltf.single_mesh();
    let mut geometries = Vec::new();
    let mut vertex_buffer_head = 0;
    let mut index_buffer_head = 0;
    for primitive in mesh.primitives() {
        let positions = primitive
            .attributes()
            .find_map(|(s, a)| if s == gltf::Semantic::Positions { Some(a) } else { None })
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
            vertex_buffer[geometry.first_vertex + i].pos[0] = pos[0];
            vertex_buffer[geometry.first_vertex + i].pos[1] = pos[1];
            vertex_buffer[geometry.first_vertex + i].pos[2] = pos[2];
        }

        let normal_reader = reader.read_normals().unwrap();
        assert!(normal_reader.len() == geometry.vertex_count);

        for (i, normal) in normal_reader.enumerate() {
            vertex_buffer[geometry.first_vertex + i].normal[0] = normal[0];
            vertex_buffer[geometry.first_vertex + i].normal[1] = normal[1];
            vertex_buffer[geometry.first_vertex + i].normal[2] = normal[2];
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
