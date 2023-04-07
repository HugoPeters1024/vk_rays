use ash::vk;
use gpu_allocator::vulkan::*;
use gpu_allocator::*;
use std::ops::{Index, IndexMut};

use crate::render_device::RenderDevice;

pub struct Buffer<T> {
    pub nr_elements: u64,
    pub usage: vk::BufferUsageFlags,
    pub handle: vk::Buffer,
    pub allocation: Allocation,
    pub address: u64,
    marker: std::marker::PhantomData<T>,
}

impl<T> Index<usize> for Buffer<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        unsafe {
            self.allocation
                .mapped_ptr()
                .expect("buffer not mappable")
                .as_ptr()
                .cast::<T>()
                .add(index)
                .as_ref()
                .unwrap()
        }
    }
}

impl<T> IndexMut<usize> for Buffer<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        unsafe {
            self.allocation
                .mapped_ptr()
                .expect("buffer not mappable")
                .as_ptr()
                .cast::<T>()
                .add(index)
                .as_mut()
                .unwrap()
        }
    }
}

pub trait BufferProvider {
    fn create_host_buffer<T>(
        &self,
        device: &RenderDevice,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> Buffer<T>;

    fn create_device_buffer<T>(
        &self,
        device: &RenderDevice,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> Buffer<T>;

    fn create_buffer<T>(
        &self,
        device: &RenderDevice,
        size: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
    ) -> Buffer<T>;

    fn upload_buffer<T>(&self, host_buffer: &Buffer<T>, device_buffer: &Buffer<T>);
}

impl BufferProvider for RenderDevice {
    fn create_host_buffer<T>(
        &self,
        device: &RenderDevice,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> Buffer<T> {
        self.create_buffer(
            device,
            size,
            usage | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::CpuToGpu,
        )
    }

    fn create_device_buffer<T>(
        &self,
        device: &RenderDevice,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> Buffer<T> {
        self.create_buffer(
            device,
            size,
            usage | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::GpuOnly,
        )
    }

    fn create_buffer<T>(
        &self,
        device: &RenderDevice,
        nr_elements: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
    ) -> Buffer<T> {
        let buffer_info = vk::BufferCreateInfo::builder()
            .size(nr_elements * std::mem::size_of::<T>() as u64)
            .usage(usage);

        let handle = unsafe { device.device.create_buffer(&buffer_info, None).unwrap() };
        let requirements = unsafe { device.device.get_buffer_memory_requirements(handle) };
        let allocation = self
            .allocator
            .lock()
            .unwrap()
            .allocate(&AllocationCreateDesc {
                name: "",
                requirements,
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .unwrap();

        unsafe {
            device
                .device
                .bind_buffer_memory(handle, allocation.memory(), allocation.offset())
                .unwrap();
        }

        let address = unsafe {
            device.device.get_buffer_device_address(
                &vk::BufferDeviceAddressInfo::builder()
                    .buffer(handle)
                    .build(),
            )
        };

        Buffer {
            handle,
            nr_elements,
            usage,
            allocation,
            address,
            marker: std::marker::PhantomData,
        }
    }

    fn upload_buffer<T>(&self, host_buffer: &Buffer<T>, device_buffer: &Buffer<T>) {
        unsafe {
            self.run_single_commands(&|cmd_buffer| {
                let copy_region = vk::BufferCopy::builder()
                    .src_offset(0)
                    .dst_offset(0)
                    .size(host_buffer.nr_elements * std::mem::size_of::<T>() as u64)
                    .build();
                self.device.cmd_copy_buffer(
                    cmd_buffer,
                    host_buffer.handle,
                    device_buffer.handle,
                    &[copy_region],
                );
            });
        }
    }
}
