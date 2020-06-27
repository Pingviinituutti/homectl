#![feature(try_blocks)]

#[macro_use]
extern crate diesel;

mod db;
mod homectl_core;
mod integrations;

use db::{actions::find_floorplans, establish_connection};
use homectl_core::{
    devices_manager::DevicesManager, events::*, groups_manager::GroupsManager,
    integrations_manager::IntegrationsManager, rules_engine::RulesEngine, scene::SceneDescriptor,
    scenes_manager::ScenesManager,
};
use std::error::Error;

// https://github.com/actix/examples/blob/master/diesel/src/main.rs
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (config, opaque_integrations_configs) = homectl_core::config::read_config();

    println!("Using config:");
    println!("{:#?}", config);

    let (sender, receiver) = mk_channel();

    let integrations_manager = IntegrationsManager::new(sender.clone());
    let groups_manager = GroupsManager::new(config.groups);
    let scenes_manager = ScenesManager::new(config.scenes, groups_manager);
    let mut devices_manager = DevicesManager::new(sender.clone(), scenes_manager);
    let rules_engine = RulesEngine::new(config.routines, sender.clone());

    for (id, integration_config) in &config.integrations {
        let opaque_integration_config: &config::Value =
            opaque_integrations_configs.get(id).unwrap();

        integrations_manager
            .load_integration(&integration_config.plugin, id, opaque_integration_config)
            .unwrap();
    }

    let connection = establish_connection();
    let results = find_floorplans(&connection);
    println!("Floorplans in DB: {:?}", results);

    let _result: Result<(), ()> = {
        integrations_manager.run_register_pass().await?;
        integrations_manager.run_start_pass().await?;

        Ok(())
    };

    loop {
        let msg = receiver.recv()?;

        // println!("got msg: {:?}", msg);

        match msg {
            Message::IntegrationDeviceRefresh { device } => {
                devices_manager.handle_integration_device_refresh(device)
            }
            Message::DeviceUpdate {
                old_state,
                new_state,
                old,
                new,
            } => rules_engine.handle_device_update(old_state, new_state, old, new),
            Message::SetDeviceState { device } => {
                devices_manager.set_device_state(&device, false);
            }
            Message::SetIntegrationDeviceState { device } => {
                integrations_manager.set_integration_device_state(device);
            }
            Message::ActivateScene(SceneDescriptor { scene_id }) => {
                devices_manager.activate_scene(&scene_id);
            }
        }
    }
}
