use crate::camera::{Camera3d, Camera3dPlugin};
use crate::rasterization_pipeline::{RasterizationPipeline, RasterizationPipelinePlugin, RasterizationRegisters};
use crate::raytracing_pipeline::{RaytracerRegisters, RaytracingPipeline, RaytracingPlugin};
use crate::render_buffer::{Buffer, BufferProvider};
use crate::scene::{Scene, ScenePlugin};
use crate::shader_binding_table::{SBTPlugin, SBT};
use crate::sphere_blas::{cleanup_sphere_blas, SphereBLAS, AABB};
use crate::vulkan_assets::{AddVulkanAsset, VkAssetCleanupPlaybook, VulkanAssets};
use crate::vulkan_cleanup::{VkCleanup, VkCleanupEvent, VkCleanupPlugin};
use crate::{render_device::RenderDevice, swapchain::Swapchain};
use crate::{swapchain, vk_utils};
use ash::vk;
use bevy::app::AppExit;
use bevy::ecs::event::ManualEventReader;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::winit::WinitSettings;
use bevy::{
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper},
};
use rand::RngCore;

#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct RenderSchedule;

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum RenderSet {
    Prepare,
    Extract,
    Render,
}

impl RenderSet {
    pub fn base_schedule() -> Schedule {
        use RenderSet::*;

        let mut schedule = Schedule::new();
        schedule.add_systems((
            flush_ecs.in_set(Prepare),
            flush_ecs.in_set(Extract),
            flush_ecs.in_set(Render),
        ));

        schedule.configure_sets((Prepare, Extract, Render).chain());
        schedule.set_executor_kind(bevy::ecs::schedule::ExecutorKind::SingleThreaded);

        schedule
    }
}

#[allow(unused_variables)]
fn flush_ecs(world: &mut World) {}

#[derive(Resource)]
pub struct RenderConfig {
    pub rt_pipeline: Handle<RaytracingPipeline>,
    pub quad_pipeline: Handle<RasterizationPipeline>,
    pub skybox: Handle<bevy::prelude::Image>,
}

#[derive(Resource, Default, Deref, DerefMut)]
pub struct RayFocalFocus(pub Option<(u32, u32)>);

#[derive(Resource)]
pub struct FrameResources {
    per_frame: Vec<RenderResources>,
    current_frame: usize,
}

impl FrameResources {
    pub fn get(&self) -> &RenderResources {
        &self.per_frame[self.current_frame]
    }

    pub fn get_mut(&mut self) -> &mut RenderResources {
        &mut self.per_frame[self.current_frame]
    }

    fn cycle(&mut self) {
        self.current_frame = (self.current_frame + 1) % self.per_frame.len();
    }

    fn current_idx(&self) -> usize {
        self.current_frame
    }
}

pub struct RenderResources {
    pub uniform_buffer: Buffer<UniformData>,
    pub query_buffer: Buffer<QueryData>,
    pub fence: vk::Fence,
    pub cmd_buffer: vk::CommandBuffer,
}

fn cleanup_render_resources(render_resources: Res<FrameResources>, cleanup: Res<VkCleanup>) {
    for res in &render_resources.per_frame {
        cleanup.send(VkCleanupEvent::Buffer(res.uniform_buffer.handle));
        cleanup.send(VkCleanupEvent::Buffer(res.query_buffer.handle));
        cleanup.send(VkCleanupEvent::Fence(res.fence));
    }
}

#[repr(C)]
pub struct UniformData {
    inverse_view: Mat4,
    inverse_proj: Mat4,
    entropy: u32,
    should_clear: u32,
    mouse_x: u32,
    mouse_y: u32,
    exposure: f32,
}

#[repr(C)]
pub struct QueryData {
    focal_distance: f32,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        // Don't ask, shit will segfault otherwise
        let mut winit_settings = app.world.get_resource_mut::<WinitSettings>().unwrap();
        winit_settings.return_from_run = true;

        let (whandles, _) = app
            .world
            .query::<(&RawHandleWrapper, With<PrimaryWindow>)>()
            .single(&mut app.world);
        let render_device = RenderDevice::from_window(whandles);
        app.world.insert_resource(render_device.clone());

        app.init_resource::<RayFocalFocus>();

        app.add_plugin(VkCleanupPlugin);

        app.world
            .insert_resource(SphereBLAS::make_one(&AABB::default(), &render_device));

        let mut render_schedule = RenderSet::base_schedule();
        render_schedule.add_system(wait_for_frame_finish.in_set(RenderSet::Prepare));
        render_schedule.add_system(render.in_set(RenderSet::Render));

        app.add_schedule(RenderSchedule, render_schedule);

        app.add_plugin(swapchain::SwapchainPlugin);
        app.add_plugin(RaytracingPlugin);
        app.add_plugin(RasterizationPipelinePlugin);
        app.add_plugin(SBTPlugin);
        app.add_plugin(Camera3dPlugin);

        app.add_system(run_render_schedule);
        app.add_system(shutdown.in_base_set(CoreSet::Last));

        app.add_asset::<crate::shader::Shader>()
            .init_asset_loader::<crate::shader::ShaderLoader>()
            .init_debug_asset_loader::<crate::shader::ShaderLoader>()
            .add_asset::<crate::render_image::Image>()
            .add_vulkan_asset::<crate::render_image::Image>()
            .add_asset::<crate::gltf_assets::GltfMesh>()
            .add_vulkan_asset::<crate::gltf_assets::GltfMesh>()
            .init_asset_loader::<crate::gltf_assets::GltfLoader>()
            .init_debug_asset_loader::<crate::gltf_assets::GltfLoader>()
            .add_vulkan_asset::<bevy::prelude::Image>();

        app.add_plugin(ScenePlugin);

        app.world
            .get_resource_mut::<VkAssetCleanupPlaybook>()
            .unwrap()
            .add_system(cleanup_render_resources)
            .add_system(cleanup_sphere_blas);

        let mk_resources = || {
            let uniform_buffer =
                render_device.create_host_buffer::<UniformData>(1, vk::BufferUsageFlags::UNIFORM_BUFFER);

            let mut query_buffer_host =
                render_device.create_host_buffer::<QueryData>(1, vk::BufferUsageFlags::TRANSFER_SRC);
            {
                let mut query_buffer_host = render_device.map_buffer(&mut query_buffer_host);
                query_buffer_host[0] = QueryData { focal_distance: 7.0 };
            }

            let query_buffer = render_device.create_device_buffer::<QueryData>(
                1,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            );
            unsafe {
                render_device.run_single_commands(|cmd_buffer| {
                    render_device.upload_buffer(cmd_buffer, &query_buffer_host, &query_buffer);
                });
            }
            app.world
                .get_resource::<VkCleanup>()
                .unwrap()
                .send(VkCleanupEvent::Buffer(query_buffer_host.handle));

            let fence_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
            let fence = unsafe { render_device.device.create_fence(&fence_info, None) }.unwrap();

            let alloc_info = vk::CommandBufferAllocateInfo::builder()
                .command_pool(render_device.command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);

            let cmd_buffer = unsafe { render_device.device.allocate_command_buffers(&alloc_info) }.unwrap()[0];

            RenderResources {
                uniform_buffer,
                query_buffer,
                fence,
                cmd_buffer,
            }
        };

        app.world.insert_resource(FrameResources {
            per_frame: vec![mk_resources(), mk_resources()],
            current_frame: 0,
        });
    }
}

fn run_render_schedule(world: &mut World) {
    world.run_schedule(RenderSchedule);
}

fn wait_for_frame_finish(
    device: Res<RenderDevice>,
    cleanup: Res<VkCleanup>,
    mut swapchain: Query<&mut Swapchain>,
    render_resources: ResMut<FrameResources>,
) {
    // get the next image to render to
    let mut swapchain = swapchain.single_mut();
    swapchain.aquire_next_image(&device);

    // TODO use two scene tlasses
    // render_resources.cycle();
    unsafe {
        device
            .device
            .wait_for_fences(std::slice::from_ref(&render_resources.get().fence), true, u64::MAX)
            .unwrap();
        device
            .device
            .reset_fences(std::slice::from_ref(&render_resources.get().fence))
            .unwrap();
    }
    cleanup.send(VkCleanupEvent::SignalNextFrame);
}

fn render(
    device: Res<RenderDevice>,
    scene: Res<Scene>,
    mut swapchain: Query<&mut Swapchain>,
    textures: Res<VulkanAssets<bevy::prelude::Image>>,
    gtransforms: Query<Ref<GlobalTransform>>,
    render_config: Res<RenderConfig>,
    mut render_resources: ResMut<FrameResources>,
    rt_pipelines: Res<VulkanAssets<RaytracingPipeline>>,
    rast_pipelines: Res<VulkanAssets<RasterizationPipeline>>,
    sbt: Res<SBT>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(Entity, &Camera3d)>,
    focal_focus: Res<RayFocalFocus>,
) {
    let mut swapchain = swapchain.single_mut();
    let (camera_e, camera) = camera.single();

    // wait for the previous frame to finish
    unsafe {
        let (swapchain_image, swapchain_view) = swapchain.current_framebuffer();

        let cmd_buffer = render_resources.get().cmd_buffer;
        device
            .device
            .reset_command_buffer(cmd_buffer, vk::CommandBufferResetFlags::empty())
            .unwrap();

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        device.device.begin_command_buffer(cmd_buffer, &begin_info).unwrap();

        swapchain.on_begin_render(cmd_buffer);

        // Make swapchain available for rendering
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            swapchain_image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        );

        if let Some(compiled) = rt_pipelines.get(&render_config.rt_pipeline) {
            if let Some(skybox) = textures.get(&render_config.skybox) {
                if scene.is_ready() {
                    let ray_descriptor_set = compiled.descriptor_sets[render_resources.current_idx()];
                    let mut writes = Vec::new();
                    // update the descriptor set
                    let render_target_image_binding = vk::DescriptorImageInfo::builder()
                        .image_layout(vk::ImageLayout::GENERAL)
                        .image_view(swapchain.render_target.view)
                        .build();

                    writes.push(
                        vk::WriteDescriptorSet::builder()
                            .dst_set(ray_descriptor_set)
                            .dst_binding(0)
                            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                            .image_info(std::slice::from_ref(&render_target_image_binding))
                            .build(),
                    );

                    let mut p_acceleration_structure_write = vk::WriteDescriptorSetAccelerationStructureKHR::builder()
                        .acceleration_structures(std::slice::from_ref(&scene.tlas.handle))
                        .build();

                    let mut write_acceleration_structure = vk::WriteDescriptorSet::builder()
                        .dst_set(ray_descriptor_set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                        .push_next(&mut p_acceleration_structure_write)
                        .build();
                    write_acceleration_structure.descriptor_count = 1;

                    writes.push(write_acceleration_structure);

                    let skybox_image_binding = vk::DescriptorImageInfo::builder()
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .image_view(skybox.view)
                        .sampler(device.linear_sampler)
                        .build();

                    writes.push(
                        vk::WriteDescriptorSet::builder()
                            .dst_set(ray_descriptor_set)
                            .dst_binding(2)
                            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                            .image_info(std::slice::from_ref(&skybox_image_binding))
                            .build(),
                    );

                    device.device.update_descriptor_sets(&writes, &[]);

                    device.device.cmd_bind_pipeline(
                        cmd_buffer,
                        vk::PipelineBindPoint::RAY_TRACING_KHR,
                        compiled.vk_pipeline,
                    );

                    {
                        let mut uniform_view = device.map_buffer(&mut render_resources.get_mut().uniform_buffer);
                        let mut rng = rand::thread_rng();
                        let camera_transform = gtransforms.get(camera_e).unwrap();
                        let (_, rotation, translation) = camera_transform.to_scale_rotation_translation();
                        let camera_view = Mat4::from_quat(rotation) * Mat4::from_translation(translation);
                        let projection = Mat4::perspective_rh(
                            camera.fov,
                            swapchain.width as f32 / swapchain.height as f32,
                            camera.min_t,
                            camera.max_t,
                        );
                        let entropy = rng.next_u32();
                        uniform_view[0] = UniformData {
                            inverse_view: camera_view.inverse(),
                            inverse_proj: projection.inverse(),
                            entropy,
                            should_clear: (focal_focus.0.is_some() || camera.moved) as u32,
                            mouse_x: focal_focus.0.map_or(0, |f| f.0),
                            mouse_y: focal_focus.0.map_or(0, |f| f.1),
                            exposure: camera.exposure,
                        };
                    }

                    let push_constants = RaytracerRegisters {
                        uniform_buffer_address: render_resources.get().uniform_buffer.address,
                        query_buffer_address: render_resources.get().query_buffer.address,
                    };

                    device.device.cmd_push_constants(
                        cmd_buffer,
                        compiled.pipeline_layout,
                        vk::ShaderStageFlags::RAYGEN_KHR,
                        0,
                        bytemuck::bytes_of(&push_constants),
                    );

                    device.device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::RAY_TRACING_KHR,
                        compiled.pipeline_layout,
                        0,
                        &[ray_descriptor_set, device.g_descriptor_set],
                        &[],
                    );

                    if sbt.data.address != 0 {
                        device.exts.rt_pipeline.cmd_trace_rays(
                            cmd_buffer,
                            &sbt.raygen_region,
                            &sbt.miss_region,
                            &sbt.hit_region,
                            &vk::StridedDeviceAddressRegionKHR::default(),
                            swapchain.width,
                            swapchain.height,
                            1,
                        )
                    }
                }
            }

            // make render target available for sampling
            vk_utils::transition_image_layout(
                &device,
                cmd_buffer,
                swapchain.render_target.handle,
                vk::ImageLayout::GENERAL,
                vk::ImageLayout::GENERAL,
            );

            if let Some(compiled) = rast_pipelines.get(&render_config.quad_pipeline) {
                let rast_descriptor_set = compiled.descriptor_sets[render_resources.current_idx()];
                // update the descriptor set
                let render_target_image_binding = vk::DescriptorImageInfo::builder()
                    .image_layout(vk::ImageLayout::GENERAL)
                    .image_view(swapchain.render_target.view)
                    .sampler(device.nearest_sampler)
                    .build();

                let descriptor_write = vk::WriteDescriptorSet::builder()
                    .dst_set(rast_descriptor_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(std::slice::from_ref(&render_target_image_binding))
                    .build();

                device
                    .device
                    .update_descriptor_sets(std::slice::from_ref(&descriptor_write), &[]);

                let render_area = vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D {
                        width: swapchain.width,
                        height: swapchain.height,
                    },
                };

                let attachment_info = vk::RenderingAttachmentInfoKHR::builder()
                    .image_view(swapchain_view)
                    .image_layout(vk::ImageLayout::ATTACHMENT_OPTIMAL)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .clear_value(vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [0.0, 0.0, 0.0, 0.0],
                        },
                    });

                let render_info = vk::RenderingInfo::builder()
                    .layer_count(1)
                    .render_area(render_area)
                    .color_attachments(std::slice::from_ref(&attachment_info));

                device.device.cmd_begin_rendering(cmd_buffer, &render_info);

                device
                    .device
                    .cmd_set_scissor(cmd_buffer, 0, std::slice::from_ref(&render_area));
                device.device.cmd_set_viewport(
                    cmd_buffer,
                    0,
                    std::slice::from_ref(&vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: swapchain.width as f32,
                        height: swapchain.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }),
                );

                device
                    .device
                    .cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::GRAPHICS, compiled.vk_pipeline);

                device.device.cmd_bind_descriptor_sets(
                    cmd_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    compiled.pipeline_layout,
                    0,
                    std::slice::from_ref(&rast_descriptor_set),
                    &[],
                );

                let push_constants = RasterizationRegisters {
                    uniforms: render_resources.get().uniform_buffer.address,
                };

                device.device.cmd_push_constants(
                    cmd_buffer,
                    compiled.pipeline_layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(&push_constants),
                );

                device.device.cmd_draw(cmd_buffer, 3, 1, 0, 0);
                device.device.cmd_end_rendering(cmd_buffer);
            }
        }

        // Make swapchain available for presentation
        vk_utils::transition_image_layout(
            &device,
            cmd_buffer,
            swapchain_image,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::ImageLayout::PRESENT_SRC_KHR,
        );

        device.device.end_command_buffer(cmd_buffer).unwrap();

        // submit the command buffer to the queue
        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(std::slice::from_ref(&cmd_buffer))
            .wait_semaphores(std::slice::from_ref(&swapchain.image_ready_sem))
            .wait_dst_stage_mask(std::slice::from_ref(&vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT))
            .signal_semaphores(std::slice::from_ref(&swapchain.render_finished_sem))
            .build();

        {
            let queue = device.queue.lock().unwrap();
            device
                .device
                .queue_submit(
                    queue.clone(),
                    std::slice::from_ref(&submit_info),
                    render_resources.get().fence,
                )
                .unwrap();
        }

        let image_idx = swapchain.current_image_idx as u32;

        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(std::slice::from_ref(&swapchain.render_finished_sem))
            .swapchains(std::slice::from_ref(&swapchain.handle))
            .image_indices(std::slice::from_ref(&image_idx))
            .build();

        let present_result = {
            let queue = device.queue.lock().unwrap();
            device.exts.swapchain.queue_present(queue.clone(), &present_info)
        };

        match present_result {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::SUBOPTIMAL_KHR) => {
                println!("------ SWAPCHAIN OUT OF DATE ------");
                let primary_window = primary_window.get_single().unwrap();
                swapchain.on_resize(primary_window);
            }
            Err(e) => panic!("Failed to present swapchain image: {:?}", e),
            Ok(_) => {}
        }
    }
}

fn shutdown(world: &mut World) {
    let mut exit_reader = ManualEventReader::<AppExit>::default();
    let exit_events = world.get_resource::<Events<AppExit>>().unwrap();

    if exit_reader.iter(exit_events).last().is_some() {
        let mut cleanup_playbook = world.remove_resource::<VkAssetCleanupPlaybook>().unwrap();
        cleanup_playbook.run(world);

        let cleanup = world.remove_resource::<VkCleanup>().unwrap();
        cleanup.flush_and_die();
    }
}
