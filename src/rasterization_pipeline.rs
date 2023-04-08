use ash::vk;
use bevy::prelude::*;
use bevy::reflect::TypeUuid;

use crate::composed_asset::ComposedAsset;
use crate::shader::Shader;

#[derive(Default, TypeUuid)]
#[uuid = "f5b5b0f0-1b5f-4b0e-9c1f-1f1b0c0c0c0c"]
pub struct RasterizationPipeline {
    pub vs_shader: Handle<Shader>,
    pub fs_shader: Handle<Shader>,
    pub compiled: Option<VkRasterizationPipeline>,
}

impl ComposedAsset for RasterizationPipeline {
    type DepType = Shader;

    fn get_deps(&self) -> Vec<&Handle<Self::DepType>> {
        vec![&self.vs_shader, &self.fs_shader]
    }
}

pub struct VkRasterizationPipeline {
}
