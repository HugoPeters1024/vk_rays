use bevy::asset::{Asset, HandleId};
use bevy::ecs::schedule::ExecutorKind;
use bevy::ecs::system::{StaticSystemParam, SystemParam, SystemParamItem};
use bevy::prelude::*;
use bevy::utils::HashMap;
use crossbeam_channel::{Receiver, Sender};

use crate::render_device::RenderDevice;
use crate::render_plugin::{RenderSchedule, RenderSet};
use crate::vulkan_cleanup::VkCleanup;

pub trait VulkanAsset: Asset {
    type ExtractedAsset: Send + Sync + 'static;
    type PreparedAsset: Send + Sync + 'static;
    type Param: SystemParam;

    fn extract_asset(
        &self,
        param: &mut SystemParamItem<Self::Param>,
    ) -> Option<Self::ExtractedAsset>;
    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset;

    fn destroy_asset(asset: Self::PreparedAsset, cleanup: &VkCleanup);
}

#[derive(Resource, Default)]
pub struct VkAssetCleanupPlaybook(Schedule);

impl VkAssetCleanupPlaybook {
    pub fn run(&mut self, world: &mut World) {
        self.0
            .set_executor_kind(ExecutorKind::SingleThreaded)
            .run(world);
    }

    pub fn add_system<M>(&mut self, system: impl IntoSystemConfig<M>) -> &mut Self {
        self.0.add_system(system);
        self
    }
}

#[derive(Resource)]
pub struct VulkanAssets<T: VulkanAsset> {
    lookup: HashMap<HandleId, T::PreparedAsset>,
    send_extracted: Sender<(HandleId, T::ExtractedAsset)>,
    recv_prepared: Receiver<(HandleId, T::PreparedAsset)>,
}

impl<T: VulkanAsset> VulkanAssets<T> {
    pub fn get(&self, handle: &Handle<T>) -> Option<&T::PreparedAsset> {
        self.lookup.get(&handle.id())
    }
}

#[derive(Default)]
pub struct VulkanAssetPlugin<T: VulkanAsset> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: VulkanAsset> Plugin for VulkanAssetPlugin<T> {
    fn build(&self, app: &mut App) {
        if app.world.get_resource::<VulkanAssets<T>>().is_some() {
            panic!("VulkanAssetPlugin already added");
        }

        app.world.init_resource::<VkAssetCleanupPlaybook>();

        let mut playbook = app
            .world
            .get_resource_mut::<VkAssetCleanupPlaybook>()
            .unwrap();
        playbook.0.add_system(destroy_vulkan_asset::<T>);

        app.add_asset::<T>();

        let (send_extracted, recv_extracted) = crossbeam_channel::unbounded();
        let (send_prepared, recv_prepared) = crossbeam_channel::unbounded();
        app.world.insert_resource(VulkanAssets::<T> {
            lookup: HashMap::default(),
            send_extracted,
            recv_prepared,
        });

        app.edit_schedule(RenderSchedule, |schedule| {
            schedule.add_system(extract_vulkan_asset::<T>.in_set(RenderSet::Extract));
            schedule.add_system(publish_vulkan_asset::<T>.in_set(RenderSet::Extract));
        });

        let render_device = app.world.get_resource::<RenderDevice>().unwrap().clone();

        std::thread::spawn(move || {
            prepare_asset::<T>(render_device, recv_extracted, send_prepared);
        });
    }
}

fn extract_vulkan_asset<T: VulkanAsset>(
    mut asset_events: EventReader<AssetEvent<T>>,
    assets: Res<Assets<T>>,
    vk_assets: Res<VulkanAssets<T>>,
    param: StaticSystemParam<T::Param>,
) {
    let mut param = param.into_inner();
    for event in asset_events.iter() {
        match event {
            AssetEvent::Created { handle } => {
                println!("{} asset created", std::any::type_name::<T>());
                let asset = assets.get(handle).unwrap();
                if let Some(extracted_asset) = asset.extract_asset(&mut param) {
                    vk_assets
                        .send_extracted
                        .send((handle.id(), extracted_asset))
                        .unwrap();
                } else {
                    println!(
                        "A {} could not be extracted for rendering...",
                        std::any::type_name::<T>()
                    );
                }
            }
            AssetEvent::Modified { handle } => {
                println!("{} asset modified", std::any::type_name::<T>());
                let asset = assets.get(handle).unwrap();
                if let Some(extracted_asset) = asset.extract_asset(&mut param) {
                    vk_assets
                        .send_extracted
                        .send((handle.id(), extracted_asset))
                        .unwrap();
                } else {
                    println!(
                        "A {} could not be extracted for rendering...",
                        std::any::type_name::<T>()
                    );
                }
            }
            AssetEvent::Removed { handle: _handle } => {
                println!("AAAAAAAAAAAAAAAAAAA AssetEvent::Removed");
            }
        }
    }
}

fn publish_vulkan_asset<T: VulkanAsset>(
    mut vk_assets: ResMut<VulkanAssets<T>>,
    cleanup: Res<VkCleanup>,
) {
    while let Ok((handle_id, prepared_asset)) = vk_assets.recv_prepared.try_recv() {
        println!(
            "{} asset received, inserting into world",
            std::any::type_name::<T::PreparedAsset>()
        );
        if let Some(old) = vk_assets.lookup.insert(handle_id, prepared_asset) {
            T::destroy_asset(old, &cleanup);
        }
    }
}

// run on the dedicated thread
fn prepare_asset<T: VulkanAsset>(
    device: RenderDevice,
    recv_extracted: Receiver<(HandleId, T::ExtractedAsset)>,
    send_prepared: Sender<(HandleId, T::PreparedAsset)>,
) {
    println!(
        "Prepare asset thread for {} started",
        std::any::type_name::<T::PreparedAsset>()
    );
    while let Ok((handle_id, extracted_asset)) = recv_extracted.recv() {
        println!(
            "{} asset received, preparing...",
            std::any::type_name::<T::PreparedAsset>()
        );
        let prepared_asset = T::prepare_asset(&device, extracted_asset);
        send_prepared.send((handle_id, prepared_asset)).unwrap();
        println!(
            "{} asset prepared, sending to main thread",
            std::any::type_name::<T::PreparedAsset>()
        );
    }
    println!(
        "Prepare asset thread for {} finished",
        std::any::type_name::<T::PreparedAsset>()
    );
}

fn destroy_vulkan_asset<T: VulkanAsset>(
    mut vk_assets: ResMut<VulkanAssets<T>>,
    cleanup: Res<VkCleanup>,
) {
    println!(
        "Destroying all vulkan assets of type {}",
        std::any::type_name::<T>()
    );
    for (_, asset) in vk_assets.lookup.drain() {
        T::destroy_asset(asset, &cleanup);
    }
}

pub trait AddVulkanAsset {
    fn add_vulkan_asset<T: VulkanAsset>(&mut self) -> &mut Self;
}

impl AddVulkanAsset for App {
    fn add_vulkan_asset<T: VulkanAsset>(&mut self) -> &mut Self {
        self.add_plugin(VulkanAssetPlugin::<T> {
            _phantom: std::marker::PhantomData,
        });

        self
    }
}
