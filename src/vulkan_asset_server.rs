use bevy::asset::{Asset, HandleId};
use bevy::ecs::system::{SystemParam, SystemParamItem, StaticSystemParam};
use bevy::prelude::*;
use bevy::utils::HashMap;
use crossbeam_channel::{Receiver, Sender};

use crate::render_device::RenderDevice;
use crate::render_plugin::{RenderSet, RenderSchedule};

pub trait VulkanAsset: Asset {
    type ExtractedAsset: Send + Sync + 'static;
    type PreparedAsset: Send + Sync + 'static;
    type Param: SystemParam;

    fn extract_asset(&self, param: &mut SystemParamItem<Self::Param>) -> Option<Self::ExtractedAsset>;
    fn prepare_asset(device: &RenderDevice, asset: Self::ExtractedAsset) -> Self::PreparedAsset;
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
                let asset = assets.get(handle).unwrap();
                if let Some(extracted_asset) = asset.extract_asset(&mut param) {
                    vk_assets.send_extracted.send((handle.id(), extracted_asset)).unwrap();
                } else {
                    println!("A {} could not be extracted for rendering...", std::any::type_name::<T>());
                }
            }
            AssetEvent::Modified { handle } => {
                let asset = assets.get(handle).unwrap();
                if let Some(extracted_asset) = asset.extract_asset(&mut param) {
                    vk_assets.send_extracted.send((handle.id(), extracted_asset)).unwrap();
                } else {
                    println!("A {} could not be extracted for rendering...", std::any::type_name::<T>());
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
) {
    while let Ok((handle_id, prepared_asset)) = vk_assets.recv_prepared.try_recv() {
        println!("{} asset received, inserting into world", std::any::type_name::<T::PreparedAsset>());
        vk_assets.lookup.insert(handle_id, prepared_asset);
    }
}

// run on the dedicated thread
fn prepare_asset<T: VulkanAsset>(
    device: RenderDevice,
    recv_extracted: Receiver<(HandleId, T::ExtractedAsset)>,
    send_prepared: Sender<(HandleId, T::PreparedAsset)>) {
    println!("Prepare asset thread for {} started", std::any::type_name::<T::PreparedAsset>());
    while let Ok((handle_id, extracted_asset)) = recv_extracted.recv() {
        let prepared_asset = T::prepare_asset(&device, extracted_asset);
        send_prepared.send((handle_id, prepared_asset)).unwrap();
    }
    println!("Prepare asset thread for {} finished", std::any::type_name::<T::PreparedAsset>());
}

pub trait AddVulkanAsset {
    fn add_vulkan_asset<T: VulkanAsset>(&mut self);
}

impl AddVulkanAsset for App {
    fn add_vulkan_asset<T: VulkanAsset>(&mut self) {
        self.add_plugin(VulkanAssetPlugin::<T> {
            _phantom: std::marker::PhantomData,
        });
    }
}

