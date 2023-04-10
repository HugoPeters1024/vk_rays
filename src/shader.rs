use std::{io::Cursor, borrow::Cow};

use crate::render_device::*;
use ash::{util::read_spv, vk};
use bevy::{
    asset::{AssetLoader, LoadedAsset},
    reflect::TypeUuid,
};
use shaderc;

#[derive(Debug, Clone, TypeUuid)]
#[uuid = "d95bc916-6c55-4de3-9622-37e7b6969fda"]
pub struct Shader {
    pub path: String,
    pub spirv: Cow<'static, [u8]>,
}

pub struct ShaderLoader {
    compiler: shaderc::Compiler,
}

impl Default for ShaderLoader {
    fn default() -> Self {
        Self {
            compiler: shaderc::Compiler::new().unwrap(),
        }
    }
}

impl AssetLoader for ShaderLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut bevy::asset::LoadContext,
    ) -> bevy::utils::BoxedFuture<'a, Result<(), bevy::asset::Error>> {
        Box::pin(async move {
            println!("Compiling shader: {:?}", load_context.path());
            let ext = load_context.path().extension().unwrap().to_str().unwrap();

            let kind = match ext {
                "vert" => shaderc::ShaderKind::Vertex,
                "frag" => shaderc::ShaderKind::Fragment,
                "comp" => shaderc::ShaderKind::Compute,
                "rgen" => shaderc::ShaderKind::RayGeneration,
                "rchit" => shaderc::ShaderKind::ClosestHit,
                "rmiss" => shaderc::ShaderKind::Miss,
                _ => panic!("Unsupported shader type: {}", ext),
            };

            let mut options = shaderc::CompileOptions::new().unwrap();
            options.set_target_env(shaderc::TargetEnv::Vulkan, vk::make_api_version(0, 1, 3, 0));
            options.set_target_spirv(shaderc::SpirvVersion::V1_6);

            let binary_result = self.compiler.compile_into_spirv(
                std::str::from_utf8(bytes).unwrap(),
                kind,
                load_context.path().to_str().unwrap(),
                "main",
                Some(&options),
            );

            let binary = match binary_result {
                Ok(binary) => binary,
                Err(e) => {
                    panic!("Shader compilation error: {}", e);
                }
            };

            let shader = Shader {
                path: load_context.path().to_str().unwrap().to_string(),
                spirv: Vec::from(binary.as_binary_u8()).into(),
            };

            load_context.set_default_asset(LoadedAsset::new(shader));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["comp", "vert", "frag", "rgen", "rchit", "rmiss"]
    }
}

pub trait ShaderProvider {
    fn load_shader(
        &self,
        shader: &Shader,
        stage: vk::ShaderStageFlags,
    ) -> vk::PipelineShaderStageCreateInfo;
}

impl ShaderProvider for RenderDevice {
    fn load_shader(
        &self,
        shader: &Shader,
        stage: vk::ShaderStageFlags,
    ) -> vk::PipelineShaderStageCreateInfo {
        let code = read_spv(&mut Cursor::new(&shader.spirv)).unwrap();
        let shader_module = unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::builder().code(&code), None)
                .unwrap()
        };

        vk::PipelineShaderStageCreateInfo::builder()
            .stage(stage)
            .module(shader_module)
            .name(std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap())
            .build()
    }
}
