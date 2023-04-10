use bevy::{asset::Asset, prelude::*, utils::HashSet};

pub trait ComposedAsset: Asset {
    type DepType: Asset;

    fn get_deps(&self) -> Vec<&Handle<Self::DepType>>;
}

pub struct ComposedAssetPlugin<T: ComposedAsset> {
    _marker: std::marker::PhantomData<T>,
}

impl<T: ComposedAsset> ComposedAssetPlugin<T> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: ComposedAsset> Plugin for ComposedAssetPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_asset::<T>();
        app.add_system(promote_composed_asset_events::<T>);
    }
}

fn promote_composed_asset_events<T: ComposedAsset>(
    dep_events: Res<Events<AssetEvent<T::DepType>>>,
    mut events_out: EventWriter<AssetEvent<T>>,
    assets: Res<Assets<T>>,
) {
    let mut dep_reader = dep_events.get_reader();
    let dep_events = dep_reader.iter(&dep_events).collect::<Vec<_>>();

    for (handle_id, asset) in assets.iter() {
        let handle: Handle<T> = Handle::weak(handle_id);
        let dependencies = asset.get_deps().into_iter().collect::<HashSet<_>>();

        let mut deps_changed = false;
        for event in &dep_events {
            match event {
                AssetEvent::Created { handle } => deps_changed |= dependencies.contains(&handle),
                AssetEvent::Modified { handle } => deps_changed |= dependencies.contains(&handle),
                AssetEvent::Removed { handle } => deps_changed |= dependencies.contains(&handle),
            }
        }

        if deps_changed {
            events_out.send(AssetEvent::Modified { handle });
        }
    }
}

pub trait ComposedAssetAppExtension {
    fn add_composed_asset<T: ComposedAsset>(&mut self) -> &mut Self;
}

impl ComposedAssetAppExtension for App {
    fn add_composed_asset<T: ComposedAsset>(&mut self) -> &mut Self {
        self.add_plugin(ComposedAssetPlugin::<T>::new());
        self
    }
}
