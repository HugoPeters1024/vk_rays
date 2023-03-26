use bevy::{
    asset::{Asset, AssetLoader},
    prelude::*,
    reflect::TypeUuid,
};
use ash::vk;
use shaderc;

#[derive(Debug, Clone, TypeUuid)]
#[uuid = "d95bc916-6c55-4de3-9622-37e7b6969fda"]
pub struct Shader {}

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
                _ => panic!("Unsupported shader type: {}", ext),
            };

            let mut options = shaderc::CompileOptions::new().unwrap();
            options.set_target_env(shaderc::TargetEnv::Vulkan, vk::make_api_version(0, 1, 3, 0));

            let binary_result = self.compiler.compile_into_spirv(std::str::from_utf8(bytes).unwrap(), kind, load_context.path().to_str().unwrap(), "main", Some(&options));

            match binary_result {
                Ok(binary) => {
                }
                Err(e) => {
                    println!("Shader compilation error: {}", e);
                }
            }

            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["comp","vert","frag"]
    }
}
