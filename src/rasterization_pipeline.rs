use ash::vk;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::prelude::*;
use bevy::reflect::TypeUuid;

use crate::composed_asset::{ComposedAsset, ComposedAssetAppExtension};
use crate::render_device::RenderDevice;
use crate::shader::{Shader, ShaderProvider};
use crate::vulkan_assets::{VulkanAsset, AddVulkanAsset};
use crate::vulkan_cleanup::{VkCleanup, VulkanCleanupEvent};

#[derive(Default, TypeUuid)]
#[uuid = "f5b5b0f0-1b5f-4b0e-9c1f-1f1b0c0c0c0c"]
pub struct RasterizationPipeline {
    pub vs_shader: Handle<Shader>,
    pub fs_shader: Handle<Shader>,
}

impl ComposedAsset for RasterizationPipeline {
    type DepType = Shader;

    fn get_deps(&self) -> Vec<&Handle<Self::DepType>> {
        vec![&self.vs_shader, &self.fs_shader]
    }
}

impl VulkanAsset for RasterizationPipeline {
    type ExtractedAsset = (Shader, Shader);
    type PreparedAsset = VkRasterizationPipeline;
    type Param = SRes<Assets<Shader>>;

    fn extract_asset(&self, shaders: &mut bevy::ecs::system::SystemParamItem<Self::Param>) -> Option<Self::ExtractedAsset> {
        let vs_shader = shaders.get(&self.vs_shader)?;
        let fs_shader = shaders.get(&self.fs_shader)?;
        Some((vs_shader.clone(), fs_shader.clone()))
    }

    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset {
        let (vs_shader, fs_shader) = asset;
        println!("creating rasterization pipeline");
        create_rast_pipeline(
            &device,
            &vs_shader,
            &fs_shader,
        )
    }

    fn destroy_asset(asset: VkRasterizationPipeline, cleanup: &VkCleanup) {
        cleanup.send(VulkanCleanupEvent::ShaderModule(asset.vert_module));
        cleanup.send(VulkanCleanupEvent::ShaderModule(asset.frag_module));
        cleanup.send(VulkanCleanupEvent::Pipeline(asset.vk_pipeline));
        cleanup.send(VulkanCleanupEvent::PipelineLayout(asset.pipeline_layout));
        cleanup.send(VulkanCleanupEvent::DescriptorSetLayout(asset.descriptor_set_layout));
    }
}

pub struct VkRasterizationPipeline {
    pub vk_pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
    pub vert_module: vk::ShaderModule,
    pub frag_module: vk::ShaderModule,
}

pub struct RasterizationPipelinePlugin;

impl Plugin for RasterizationPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.add_composed_asset::<RasterizationPipeline>();
        app.add_vulkan_asset::<RasterizationPipeline>();
    }
}

fn create_rast_pipeline(
    device: &RenderDevice,
    vs: &Shader,
    fs: &Shader,
) -> VkRasterizationPipeline {
    let shader_stages = [
        device.load_shader(&vs, vk::ShaderStageFlags::VERTEX),
        device.load_shader(&fs, vk::ShaderStageFlags::FRAGMENT),
    ];

    let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default();
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::builder()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        .primitive_restart_enable(false);

    let dynamic_state = vk::PipelineDynamicStateCreateInfo::builder()
        .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);

    let scissors = [vk::Rect2D::default()];
    let viewports = [vk::Viewport::default()];

    let viewport_state = vk::PipelineViewportStateCreateInfo::builder()
        .scissors(&scissors)
        .viewports(&viewports);

    let rasterizer = vk::PipelineRasterizationStateCreateInfo::builder()
        .line_width(1.0)
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE);

    let multisampling = vk::PipelineMultisampleStateCreateInfo::builder()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);

    let color_blend_attachment = vk::PipelineColorBlendAttachmentState::builder()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(false)
        .build();

    let color_blending = vk::PipelineColorBlendStateCreateInfo::builder()
        .attachments(std::slice::from_ref(&color_blend_attachment));

    let (descriptor_set_layout, descriptor_set) = create_rast_descriptor_data(device);

    let layout_info = vk::PipelineLayoutCreateInfo::builder()
        .set_layouts(std::slice::from_ref(&descriptor_set_layout));
    let pipeline_layout =
        unsafe { device.device.create_pipeline_layout(&layout_info, None) }.unwrap();

    let pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
        .stages(&shader_stages)
        .vertex_input_state(&vertex_input_info)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterizer)
        .multisample_state(&multisampling)
        .color_blend_state(&color_blending)
        .dynamic_state(&dynamic_state)
        .layout(pipeline_layout);

    let pipeline = unsafe {
        device.device.create_graphics_pipelines(
            vk::PipelineCache::null(),
            &[pipeline_info.build()],
            None,
        )
    }
    .unwrap()[0];

    VkRasterizationPipeline {
        vk_pipeline: pipeline,
        pipeline_layout,
        descriptor_set_layout,
        descriptor_set,
        vert_module: shader_stages[0].module,
        frag_module: shader_stages[1].module,
    }
}

fn create_rast_descriptor_data(
    device: &RenderDevice,
) -> (vk::DescriptorSetLayout, vk::DescriptorSet) {
    let sampler_layout_binding = vk::DescriptorSetLayoutBinding::builder()
        .binding(0)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        .build();

    let layout_info = vk::DescriptorSetLayoutCreateInfo::builder()
        .bindings(std::slice::from_ref(&sampler_layout_binding));

    let layout = unsafe {
        device
            .device
            .create_descriptor_set_layout(&layout_info, None)
            .unwrap()
    };

    let set = unsafe {
        device.device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(device.descriptor_pool)
                .set_layouts(std::slice::from_ref(&layout)),
        ).unwrap()[0]
    };

    (layout, set)
}
