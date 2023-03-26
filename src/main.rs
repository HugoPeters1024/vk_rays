mod render_device;
mod render_plugin;
mod swapchain;
mod shader;
mod initializers;
use crate::render_device::RenderDevice;
use crate::render_plugin::RenderPlugin;
use bevy::prelude::*;
use shader::Shader;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(RenderPlugin)
        .add_startup_system(startup)
        .add_system(test)
        .run();
}

fn startup(assets: Res<AssetServer>) {
    let shader: Handle<Shader> = assets.load("shaders/test.comp");
}

fn test(keyboard: Res<Input<KeyCode>>, time: Res<Time>, device: Res<RenderDevice>) {
    if keyboard.just_pressed(KeyCode::F) {
        println!(
            "[{}] fps: {}",
            device.device_name(),
            1.0 / time.delta_seconds()
        );
    }
}
