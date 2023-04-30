use bevy::{prelude::*, asset::{AssetLoader, LoadedAsset}, reflect::TypeUuid};

#[derive(TypeUuid, Default)]
#[uuid = "ddd211b2-ba53-47d2-a40b-15fd29d757c6"]
pub struct Gltf {
    document: Option<gltf::Document>,
    buffers: Vec<gltf::buffer::Data>,
    images: Vec<gltf::image::Data>,
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

            let asset = Gltf {
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
