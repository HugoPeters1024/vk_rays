use ash::vk;
use bevy::{
    asset::{AssetLoader, LoadedAsset},
    reflect::TypeUuid,
    utils::{HashMap, HashSet},
};

use crate::{
    acceleration_structure::{allocate_acceleration_structure, TriangleBLAS, TriangleMaterial, Vertex},
    render_buffer::{Buffer, BufferProvider},
    render_device::RenderDevice,
    render_image::VkImage,
    texture::{load_texture_from_bytes, padd_pixel_bytes_rgba_unorm},
    vulkan_assets::VulkanAsset,
    vulkan_cleanup::{VkCleanup, VkCleanupEvent}, vk_utils,
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
    type ExtractParam = ();

    fn extract_asset(
        &self,
        _param: &mut bevy::ecs::system::SystemParamItem<Self::ExtractParam>,
    ) -> Option<Self::ExtractedAsset> {
        Some(self.clone())
    }

    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        let mesh = asset.single_mesh();
        let (vertex_count, index_count) = extract_mesh_sizes(&mesh);
        let as_propeties = vk_utils::get_acceleration_structure_properties(device);

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

        let mut geometry_to_index_offset_host: Buffer<u32> = device.create_host_buffer(
            mesh.primitives().len() as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
        );

        let geometries_descrs = extract_mesh_data(
            &asset,
            vertex_buffer_view.as_slice_mut(),
            index_buffer_view.as_slice_mut(),
        );

        assert!(geometries_descrs.len() == geometry_to_index_offset_host.nr_elements as usize);

        let mut geometry_to_index_offset_view = device.map_buffer(&mut geometry_to_index_offset_host);
        for (i, g) in geometries_descrs.iter().enumerate() {
            geometry_to_index_offset_view[i] = g.first_index as u32;
        }

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

        let geometry_to_index_offset_device: Buffer<u32> = device.create_device_buffer(
            mesh.primitives().len() as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
        );

        device.run_asset_commands(|cmd_buffer| {
            device.upload_buffer(cmd_buffer, &mut vertex_buffer_host, &vertex_buffer_device);
            device.upload_buffer(cmd_buffer, &mut index_buffer_host, &index_buffer_device);
            device.upload_buffer(
                cmd_buffer,
                &mut geometry_to_index_offset_host,
                &geometry_to_index_offset_device,
            );
        });

        device.destroy_buffer(vertex_buffer_host);
        device.destroy_buffer(index_buffer_host);
        device.destroy_buffer(geometry_to_index_offset_host);

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
                            .max_vertex(0)
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
            .flags(
                vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE
                    | vk::BuildAccelerationStructureFlagsKHR::ALLOW_COMPACTION,
            )
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

        let mut acceleration_structure =
            allocate_acceleration_structure(&device, vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL, &geometry_sizes);

        let scratch_alignment = as_propeties.min_acceleration_structure_scratch_offset_alignment as u64;
        let scratch_buffer: Buffer<u8> =
            device.create_device_buffer(geometry_sizes.build_scratch_size + scratch_alignment, vk::BufferUsageFlags::STORAGE_BUFFER);

        let build_geometry_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(
                vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE
                    | vk::BuildAccelerationStructureFlagsKHR::ALLOW_COMPACTION,
            )
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .dst_acceleration_structure(acceleration_structure.handle)
            .geometries(&geometry_infos)
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: scratch_buffer.address + scratch_alignment - scratch_buffer.address % scratch_alignment,
            })
            .build();

        let build_ranges: Vec<vk::AccelerationStructureBuildRangeInfoKHR> = geometries_descrs
            .iter()
            .map(|geometry| {
                vk::AccelerationStructureBuildRangeInfoKHR::builder()
                    .primitive_count((geometry.index_count / 3) as u32)
                    // offset in bytes where the primitive data is defined
                    .primitive_offset(geometry.first_index as u32 * std::mem::size_of::<u32>() as u32)
                    .first_vertex(0)
                    .transform_offset(0)
                    .build()
            })
            .collect();

        let singleton_build_ranges = &[build_ranges.as_slice()];

        unsafe {
            device.run_asset_commands(&|cmd_buffer| {
                device.exts.rt_acc_struct.cmd_build_acceleration_structures(
                    cmd_buffer,
                    std::slice::from_ref(&build_geometry_info),
                    singleton_build_ranges,
                );
            })
        }

        device.destroy_buffer(scratch_buffer);

        let query_pool_info = vk::QueryPoolCreateInfo::builder()
            .query_type(vk::QueryType::ACCELERATION_STRUCTURE_COMPACTED_SIZE_KHR)
            .query_count(1);

        let query_pool = unsafe { device.device.create_query_pool(&query_pool_info, None) }.unwrap();
        unsafe {
            device.run_asset_commands(&|cmd_buffer| {
                device.device.cmd_reset_query_pool(cmd_buffer, query_pool, 0, 1);
            })
        }

        unsafe {
            device.run_asset_commands(&|cmd_buffer| {
                device.exts.rt_acc_struct.cmd_write_acceleration_structures_properties(
                    cmd_buffer,
                    std::slice::from_ref(&acceleration_structure.handle),
                    vk::QueryType::ACCELERATION_STRUCTURE_COMPACTED_SIZE_KHR,
                    query_pool,
                    0,
                );
            })
        }

        let mut compacted_sizes = [0];
        unsafe {
            device
                .device
                .get_query_pool_results::<u64>(query_pool, 0, 1, &mut compacted_sizes, vk::QueryResultFlags::WAIT)
                .unwrap();
        };

        println!(
            "BLAS compaction: {} -> {} ({}%)",
            geometry_sizes.acceleration_structure_size,
            compacted_sizes[0],
            (compacted_sizes[0] as f32 / geometry_sizes.acceleration_structure_size as f32) * 100.0
        );

        let compacted_buffer = device.create_device_buffer::<u8>(
            compacted_sizes[0],
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR,
        );

        let compacted_as_info = vk::AccelerationStructureCreateInfoKHR::builder()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .size(compacted_sizes[0])
            .buffer(compacted_buffer.handle)
            .build();

        let compacted_as = unsafe {
            device
                .exts
                .rt_acc_struct
                .create_acceleration_structure(&compacted_as_info, None)
        }
        .unwrap();

        unsafe {
            device.run_asset_commands(&|cmd_buffer| {
                let copy_info = vk::CopyAccelerationStructureInfoKHR::builder()
                    .src(acceleration_structure.handle)
                    .dst(compacted_as)
                    .mode(vk::CopyAccelerationStructureModeKHR::COMPACT)
                    .build();
                device
                    .exts
                    .rt_acc_struct
                    .cmd_copy_acceleration_structure(cmd_buffer, &copy_info);
            })
        }

        unsafe {
            device
                .exts
                .rt_acc_struct
                .destroy_acceleration_structure(acceleration_structure.handle, None);
            device.destroy_buffer(acceleration_structure.buffer);
            device.device.destroy_query_pool(query_pool, None);
        }
        acceleration_structure.buffer = compacted_buffer;
        acceleration_structure.handle = compacted_as;
        acceleration_structure.address = unsafe {
            device.exts.rt_acc_struct.get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR::builder()
                    .acceleration_structure(acceleration_structure.handle)
                    .build(),
            )
        };

        let mut geometry_to_material_host = device
            .create_host_buffer::<TriangleMaterial>(geometries_descrs.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC);
        let mut geometry_to_material_host_view = device.map_buffer(&mut geometry_to_material_host);
        let mut loaded_textures: HashMap<usize, VkImage> = HashMap::new();

        let mut load_cached_texture = |image_idx: usize| {
            if let Some(res) = loaded_textures.get(&image_idx) {
                return device.get_texture_descriptor_index(res.view);
            }

            let Some(image) = load_gltf_texture(&device, &asset, image_idx) else {
                return 0xFFFFFFFF;
            };

            loaded_textures.insert(image_idx, image);
            return device.get_texture_descriptor_index(loaded_textures.get(&image_idx).unwrap().view);
        };

        for (geometry_id, primitive) in mesh.primitives().enumerate() {
            geometry_to_material_host_view[geometry_id] = TriangleMaterial {
                diffuse_factor: [1.0; 4],
                diffuse_texture: 0xFFFFFFFF,
                normal_texture: 0xFFFFFFFF,
                metallic_factor: primitive.material().pbr_metallic_roughness().metallic_factor(),
                roughness_factor: primitive.material().pbr_metallic_roughness().roughness_factor(),
                metallic_roughness_texture: 0xFFFFFFFF,
            };

            if let Some(diffuse_texture) = primitive.material().pbr_metallic_roughness().base_color_texture() {
                geometry_to_material_host_view[geometry_id].diffuse_texture =
                    load_cached_texture(diffuse_texture.texture().source().index());
            }

            if let Some(normal_texture) = primitive.material().normal_texture() {
                geometry_to_material_host_view[geometry_id].normal_texture =
                    load_cached_texture(normal_texture.texture().source().index());
            }

            if let Some(metallic_rougness_texture) = primitive
                .material()
                .pbr_metallic_roughness()
                .metallic_roughness_texture()
            {
                geometry_to_material_host_view[geometry_id].metallic_roughness_texture =
                    load_cached_texture(metallic_rougness_texture.texture().source().index());
            }
        }

        let geometry_to_material_device = device.create_device_buffer::<TriangleMaterial>(
            geometry_to_material_host.nr_elements,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER,
        );

        device.run_asset_commands(|cmd_buffer| {
            device.upload_buffer(cmd_buffer, &geometry_to_material_host, &geometry_to_material_device);
        });
        device.destroy_buffer(geometry_to_material_host);

        let blas = TriangleBLAS {
            vertex_buffer: vertex_buffer_device,
            index_buffer: index_buffer_device,
            geometry_to_index_offset: geometry_to_index_offset_device,
            geometry_to_material: geometry_to_material_device,
            acceleration_structure,
            textures: loaded_textures.drain().map(|(_, v)| v).collect(),
        };

        blas
    }

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &VkCleanup) {
        for texture in asset.textures {
            cleanup.send(VkCleanupEvent::ImageView(texture.view));
            cleanup.send(VkCleanupEvent::Image(texture.handle));
        }
        cleanup.send(VkCleanupEvent::Buffer(asset.vertex_buffer.handle));
        cleanup.send(VkCleanupEvent::Buffer(asset.index_buffer.handle));
        cleanup.send(VkCleanupEvent::Buffer(asset.geometry_to_index_offset.handle));
        cleanup.send(VkCleanupEvent::Buffer(asset.geometry_to_material.handle));
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
            if normal[0].is_nan() || normal[1].is_nan() || normal[2].is_nan() {
                vertex_buffer[geometry.first_vertex + i].normal[0] = 0.0;
                vertex_buffer[geometry.first_vertex + i].normal[1] = 0.0;
                vertex_buffer[geometry.first_vertex + i].normal[2] = 0.0;
                continue;
            }

            if (1.0 - (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt()).abs() > 0.01 {
                vertex_buffer[geometry.first_vertex + i].normal[0] = 1.0;
                vertex_buffer[geometry.first_vertex + i].normal[1] = 0.0;
                vertex_buffer[geometry.first_vertex + i].normal[2] = 0.0;
                continue;
            }

            vertex_buffer[geometry.first_vertex + i].normal[0] = normal[0];
            vertex_buffer[geometry.first_vertex + i].normal[1] = normal[1];
            vertex_buffer[geometry.first_vertex + i].normal[2] = normal[2];
        }

        if let Some(uv_reader) = reader.read_tex_coords(0).map(|r| r.into_f32()) {
            for (i, uv) in uv_reader.enumerate() {
                vertex_buffer[geometry.first_vertex + i].uv[0] = uv[0];
                vertex_buffer[geometry.first_vertex + i].uv[1] = uv[1];
            }
        }

        let index_reader = reader.read_indices().unwrap().into_u32();
        assert!(index_reader.len() == geometry.index_count);
        assert!(geometry.index_count % 3 == 0);

        for (i, index) in index_reader.enumerate() {
            index_buffer[geometry.first_index + i] = index + vertex_buffer_head as u32;
        }

        vertex_buffer_head += geometry.vertex_count;
        index_buffer_head += geometry.index_count;
        geometries.push(geometry);
    }

    geometries
}

fn load_gltf_texture(device: &RenderDevice, asset: &GltfMesh, image_idx: usize) -> Option<VkImage> {
    let image = &asset.images[image_idx];
    let (bytes, format) = match image.format {
        gltf::image::Format::R8G8B8A8 => (image.pixels.clone(), vk::Format::R8G8B8A8_UNORM),
        gltf::image::Format::R8G8B8 => (
            padd_pixel_bytes_rgba_unorm(&image.pixels, 3, image.width as usize, image.height as usize),
            vk::Format::R8G8B8A8_UNORM,
        ),
        _ => {
            println!("WARNING: Unsupported texture format {:?}, ignoring...", image.format);
            return None;
        }
    };

    Some(load_texture_from_bytes(
        device,
        format,
        &bytes,
        image.width,
        image.height,
    ))
}
