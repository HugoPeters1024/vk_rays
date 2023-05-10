use crate::render_device::*;
use ash::{util::read_spv, vk};
use bevy::{
    asset::{AssetLoader, LoadedAsset},
    reflect::TypeUuid,
};
use shaderc;
use std::{borrow::Cow, fs::read_to_string, io::Cursor};

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
            let ext = load_context.path().extension().unwrap().to_str().unwrap().to_string();

            let Some(kind) = (match ext.as_str() {
                "vert" => Some(shaderc::ShaderKind::Vertex),
                "frag" => Some(shaderc::ShaderKind::Fragment),
                "comp" => Some(shaderc::ShaderKind::Compute),
                "rgen" => Some(shaderc::ShaderKind::RayGeneration),
                "rint" => Some(shaderc::ShaderKind::Intersection),
                "rchit" => Some(shaderc::ShaderKind::ClosestHit),
                "rmiss" => Some(shaderc::ShaderKind::Miss),
                _ => None,
            }) else {
                return Err(bevy::asset::Error::new(shaderc::Error::InvalidStage(format!("Unknown shader extension: {}", ext))));
            };

            let mut options = shaderc::CompileOptions::new().unwrap();
            options.set_target_env(shaderc::TargetEnv::Vulkan, vk::make_api_version(0, 1, 3, 0));
            options.set_target_spirv(shaderc::SpirvVersion::V1_6);

            options.set_include_callback(|fname, _type, _, _depth| {
                let full_path = format!("./assets/shaders/{}", fname);
                let Ok(contents) = read_to_string(full_path.clone()) else {
                    return Err(format!("Failed to read shader include: {}", fname));
                };

                Ok(shaderc::ResolvedInclude {
                    resolved_name: fname.to_string(),
                    content: contents,
                })
            });

            let binary_result = self.compiler.compile_into_spirv(
                std::str::from_utf8(bytes).unwrap(),
                kind,
                load_context.path().to_str().unwrap(),
                "main",
                Some(&options),
            );

            let Ok(binary) = binary_result else {
                let e = binary_result.err().unwrap();
                return Err(bevy::asset::Error::new(e));
            };

            let shader = Shader {
                path: load_context.path().to_str().unwrap().to_string(),
                spirv: Vec::from(binary.as_binary_u8()).into(),
            };

            let asset = LoadedAsset::new(shader);
            load_context.set_default_asset(asset);
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["comp", "vert", "frag", "rgen", "rchit", "rint", "rmiss", "glsl"]
    }
}

pub trait ShaderProvider {
    fn load_shader(&self, shader: &Shader, stage: vk::ShaderStageFlags) -> vk::PipelineShaderStageCreateInfo;
}

impl ShaderProvider for RenderDevice {
    fn load_shader(&self, shader: &Shader, stage: vk::ShaderStageFlags) -> vk::PipelineShaderStageCreateInfo {
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
