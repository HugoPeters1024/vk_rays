use bevy::{prelude::*, asset::{AssetLoader, LoadedAsset}, reflect::TypeUuid};

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

            println!("GLTF {} has {} chunks of buffer data", load_context.path().display(), asset.buffers.len());
            println!("GLTF {} has {} chunks of image data", load_context.path().display(), asset.images.len());

            load_context.set_default_asset(LoadedAsset::new(asset));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["gltf", "glb"]
    }
}
