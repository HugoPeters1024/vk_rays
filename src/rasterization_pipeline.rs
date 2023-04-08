use ash::vk;
use bevy::prelude::*;
use bevy::reflect::TypeUuid;

use crate::composed_asset::{ComposedAsset, ComposedAssetAppExtension, ComposedAssetEvent};
use crate::render_device::RenderDevice;
use crate::render_plugin::{RenderSchedule, RenderSet};
use crate::shader::{Shader, ShaderProvider};

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
    pub vk_pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
}

pub struct RasterizationPipelinePlugin;

impl Plugin for RasterizationPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.add_composed_asset::<RasterizationPipeline>();
        app.edit_schedule(RenderSchedule, |schedule| {
            schedule.add_system(ensure_rast_pipeline_up_to_date.in_set(RenderSet::Extract));
        });
    }
}

fn ensure_rast_pipeline_up_to_date(
    device: Res<RenderDevice>,
    shaders: Res<Assets<Shader>>,
    mut pipeline_events: EventReader<ComposedAssetEvent<RasterizationPipeline>>,
    mut pipelines: ResMut<Assets<RasterizationPipeline>>,
) {
    for event in pipeline_events.iter() {
        let handle = match event {
            ComposedAssetEvent(AssetEvent::Created { handle }) => handle,
            ComposedAssetEvent(AssetEvent::Modified { handle }) => handle,
            ComposedAssetEvent(AssetEvent::Removed { handle: _ }) => panic!("TODO"),
        };

        let mut pipeline = pipelines.get_mut(handle).unwrap();
        let vs_shader = shaders.get(&pipeline.vs_shader);
        let fs_shader = shaders.get(&pipeline.fs_shader);

        if vs_shader.is_none() || fs_shader.is_none() {
            println!("Not all shaders loaded, skipping pipeline reload");
            continue;
        }

        println!("creating rasterization pipeline");

        pipeline.compiled = Some(create_rast_pipeline(
            &device,
            vs_shader.unwrap(),
            fs_shader.unwrap(),
        ));
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
