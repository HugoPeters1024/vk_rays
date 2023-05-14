use ash::vk;
use gpu_allocator::vulkan::*;
use gpu_allocator::*;
use std::ops::{Index, IndexMut};

use crate::render_device::RenderDevice;

pub struct Buffer<T> {
    pub nr_elements: u64,
    pub usage: vk::BufferUsageFlags,
    pub handle: vk::Buffer,
    pub address: u64,
    marker: std::marker::PhantomData<T>,
}

impl<T> Default for Buffer<T> {
    fn default() -> Self {
        Buffer {
            nr_elements: 0,
            usage: vk::BufferUsageFlags::empty(),
            handle: vk::Buffer::null(),
            address: 0,
            marker: std::marker::PhantomData,
        }
    }
}

pub struct BufferView<T> {
    pub nr_elements: u64,
    ptr: *mut T,
    marker: std::marker::PhantomData<T>,
}

impl<T> BufferView<T> {
    pub fn as_slice_mut(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.nr_elements as usize) }
    }

    pub fn as_ptr_mut(&mut self) -> *mut T {
        self.ptr
    }
}

impl<'a, T> Index<usize> for BufferView<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        unsafe { self.ptr.add(index).as_ref().unwrap() }
    }
}

impl<'a, T> IndexMut<usize> for BufferView<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        unsafe { self.ptr.add(index).as_mut().unwrap() }
    }
}

pub trait BufferProvider {
    fn create_host_buffer<T>(&self, size: u64, usage: vk::BufferUsageFlags) -> Buffer<T>;

    fn create_device_buffer<T>(&self, size: u64, usage: vk::BufferUsageFlags) -> Buffer<T>;

    fn create_buffer<T>(&self, size: u64, usage: vk::BufferUsageFlags, location: MemoryLocation) -> Buffer<T>;

    fn upload_buffer<T>(&self, cmd_buffer: vk::CommandBuffer, host_buffer: &Buffer<T>, device_buffer: &Buffer<T>);

    fn map_buffer<T>(&self, buffer: &mut Buffer<T>) -> BufferView<T>;

    fn destroy_buffer<T>(&self, buffer: Buffer<T>);
}

impl BufferProvider for RenderDevice {
    fn create_host_buffer<T>(&self, size: u64, usage: vk::BufferUsageFlags) -> Buffer<T> {
        self.create_buffer(
            size,
            usage | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::CpuToGpu,
        )
    }

    fn create_device_buffer<T>(&self, size: u64, usage: vk::BufferUsageFlags) -> Buffer<T> {
        self.create_buffer(
            size,
            usage | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::GpuOnly,
        )
    }

    fn create_buffer<T>(&self, nr_elements: u64, usage: vk::BufferUsageFlags, location: MemoryLocation) -> Buffer<T> {
        if nr_elements == 0 {
            return Buffer {
                nr_elements,
                usage,
                handle: vk::Buffer::null(),
                address: 0,
                marker: std::marker::PhantomData,
            };
        }
        let buffer_info = vk::BufferCreateInfo::builder()
            .size(nr_elements * std::mem::size_of::<T>() as u64)
            .usage(usage);

        let handle = unsafe { self.device.create_buffer(&buffer_info, None).unwrap() };
        let requirements = unsafe { self.device.get_buffer_memory_requirements(handle) };

        {
            let mut alloc_impl = self.write_alloc();
            let allocation = alloc_impl
                .allocator
                .allocate(&AllocationCreateDesc {
                    name: "",
                    requirements,
                    location,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .unwrap();

            unsafe {
                self.device
                    .bind_buffer_memory(handle, allocation.memory(), allocation.offset())
                    .unwrap();
            }

            alloc_impl.buffer_to_allocation.insert(handle, allocation);
        }

        let address = unsafe {
            self.device
                .get_buffer_device_address(&vk::BufferDeviceAddressInfo::builder().buffer(handle).build())
        };

        Buffer {
            handle,
            nr_elements,
            usage,
            address,
            marker: std::marker::PhantomData,
        }
    }

    fn upload_buffer<T>(&self, cmd_buffer: vk::CommandBuffer, host_buffer: &Buffer<T>, device_buffer: &Buffer<T>) {
        unsafe {
            let copy_region = vk::BufferCopy::builder()
                .src_offset(0)
                .dst_offset(0)
                .size(host_buffer.nr_elements * std::mem::size_of::<T>() as u64)
                .build();
            self.device
                .cmd_copy_buffer(cmd_buffer, host_buffer.handle, device_buffer.handle, &[copy_region]);
        }
    }

    fn map_buffer<T>(&self, buffer: &mut Buffer<T>) -> BufferView<T> {
        let alloc = self.read_alloc();
        let ptr = alloc
            .buffer_to_allocation
            .get(&buffer.handle)
            .unwrap()
            .mapped_ptr()
            .unwrap()
            .as_ptr()
            .cast::<T>();
        drop(alloc);

        BufferView {
            nr_elements: buffer.nr_elements,
            ptr,
            marker: std::marker::PhantomData,
        }
    }

    fn destroy_buffer<T>(&self, buffer: Buffer<T>) {
        let mut alloc_info = self.write_alloc();
        if let Some(allocation) = alloc_info.buffer_to_allocation.remove(&buffer.handle) {
            alloc_info.allocator.free(allocation).unwrap();
        }
        unsafe {
            self.device.destroy_buffer(buffer.handle, None);
        }
    }
}

impl<T> Drop for Buffer<T> {
    fn drop(&mut self) {}
}
