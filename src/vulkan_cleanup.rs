use ash::vk;
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::render_device::RenderDevice;

#[derive(Debug)]
pub enum VkCleanupEvent {
    SignalShutdown,
    SignalNextFrame,
    DescriptorSetLayout(vk::DescriptorSetLayout),
    PipelineLayout(vk::PipelineLayout),
    Pipeline(vk::Pipeline),
    Buffer(vk::Buffer),
    ImageView(vk::ImageView),
    Image(vk::Image),
    Semaphore(vk::Semaphore),
    Fence(vk::Fence),
    ShaderModule(vk::ShaderModule),
    Swapchain(vk::SwapchainKHR),
    AccelerationStructure(vk::AccelerationStructureKHR),
}

impl VkCleanupEvent {
    fn execute(self, device: &RenderDevice) {
        println!("Executing cleanup event: {:?}", self);
        match self {
            VkCleanupEvent::DescriptorSetLayout(layout) => unsafe {
                device.device.destroy_descriptor_set_layout(layout, None);
            },
            VkCleanupEvent::PipelineLayout(layout) => unsafe {
                device.device.destroy_pipeline_layout(layout, None);
            },
            VkCleanupEvent::Pipeline(pipeline) => unsafe {
                device.device.destroy_pipeline(pipeline, None);
            },
            VkCleanupEvent::Buffer(buffer) => {
                let mut alloc_info = device.write_alloc();
                if let Some(allocation) = alloc_info.buffer_to_allocation.remove(&buffer) {
                    alloc_info.allocator.free(allocation).unwrap();
                }
                unsafe {
                    device.device.destroy_buffer(buffer, None);
                }
            }
            VkCleanupEvent::ImageView(image_view) => unsafe {
                device.device.destroy_image_view(image_view, None);
            },
            VkCleanupEvent::Image(image) => {
                let mut alloc_info = device.write_alloc();
                if let Some(allocation) = alloc_info.image_to_allocation.remove(&image) {
                    alloc_info.allocator.free(allocation).unwrap();
                }
                unsafe {
                    device.device.destroy_image(image, None);
                }
            }
            VkCleanupEvent::Semaphore(semaphore) => unsafe {
                device.device.destroy_semaphore(semaphore, None);
            },
            VkCleanupEvent::Fence(fence) => unsafe {
                device.device.destroy_fence(fence, None);
            },
            VkCleanupEvent::ShaderModule(shader_module) => unsafe {
                device.device.destroy_shader_module(shader_module, None);
            },
            VkCleanupEvent::Swapchain(swapchain) => unsafe {
                device.exts.swapchain.destroy_swapchain(swapchain, None);
            },
            VkCleanupEvent::AccelerationStructure(acceleration_structure) => unsafe {
                device
                    .exts
                    .rt_acc_struct
                    .destroy_acceleration_structure(acceleration_structure, None);
            },
            _ => panic!("Signal events should not be here"),
        }
    }
}

#[derive(Resource, Clone, Deref, DerefMut)]
pub struct VkCleanup(Arc<VkCleanupImpl>);

#[derive(Resource)]
pub struct VkCleanupImpl {
    send: Sender<VkCleanupEvent>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl VkCleanup {
    pub fn new(device: RenderDevice) -> Self {
        let (send, recv) = crossbeam_channel::unbounded();

        let device_clone = device.clone();
        let thread = std::thread::spawn(move || {
            vulkan_cleanup_worker(device_clone, recv);
        });

        Self(Arc::new(VkCleanupImpl {
            send,
            thread: Mutex::new(Some(thread)),
        }))
    }

    pub fn send(&self, event: VkCleanupEvent) {
        self.send.send(event).unwrap();
    }
}

impl VkCleanupImpl {
    pub fn flush_and_die(&self) {
        println!("Flushing the cleanup thread");
        self.send.send(VkCleanupEvent::SignalShutdown).unwrap();
        println!("Waiting for the cleanup thread to finish...");
        self.thread.lock().unwrap().take().unwrap().join().unwrap();
    }
}

fn vulkan_cleanup_worker(device: RenderDevice, recv: Receiver<VkCleanupEvent>) {
    println!("Vulkan cleanup thread started");
    let mut cycle_buffer: VecDeque<Vec<VkCleanupEvent>> = VecDeque::new();
    for _ in 0..3 {
        cycle_buffer.push_back(Vec::new());
    }

    while let Ok(event) = recv.recv() {
        match event {
            VkCleanupEvent::SignalShutdown => {
                println!("Vulkan cleanup thread received shutdown signal, flushing the destruction queue...");
                device.wait_idle();
                for events in cycle_buffer.drain(..) {
                    for event in events {
                        event.execute(&device);
                    }
                }
                break;
            }
            VkCleanupEvent::SignalNextFrame => {
                let mut events = cycle_buffer.pop_front().unwrap();
                if events.len() > 0 {
                    println!(
                        "Vulkan cleanup thread received next frame signal, flushing {} old events",
                        events.len()
                    );
                }
                for event in events.drain(..) {
                    event.execute(&device);
                }
                cycle_buffer.push_back(events);
            }
            event => {
                cycle_buffer.back_mut().unwrap().push(event);
            }
        }
    }

    println!("Vulkan cleanup thread finished");
}

pub struct VkCleanupPlugin;

impl Plugin for VkCleanupPlugin {
    fn build(&self, app: &mut App) {
        let render_device = app.world.get_resource::<RenderDevice>().unwrap().clone();
        app.insert_resource(VkCleanup::new(render_device));
    }
}
