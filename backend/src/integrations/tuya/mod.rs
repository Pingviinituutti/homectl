use anyhow::{anyhow, Context, Result};
use async_std::sync::RwLock;
use async_std::task;
use async_std::{stream, task::JoinHandle};
use async_trait::async_trait;
use futures::StreamExt;
use homectl_types::device::CorrelatedColorTemperature;
use homectl_types::{
    device::{Device, DeviceId, DeviceState, Light},
    event::{Message, TxEventChannel},
    integration::{Integration, IntegrationActionPayload, IntegrationId},
};
use palette::Hsv;
use rust_async_tuyapi::{tuyadevice::TuyaDevice, Payload, PayloadStruct};
use serde::Deserialize;
use serde_json::{json, Value};
use std::ops::Range;
use std::{
    collections::HashMap,
    net::IpAddr,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug, Deserialize)]
pub struct TuyaDeviceConfig {
    name: String,
    local_key: String,
    ip: String,
    power_on_field: String,
    brightness_field: String,
    color_temp_field: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TuyaConfig {
    devices: HashMap<DeviceId, TuyaDeviceConfig>,
}

pub struct Tuya {
    id: IntegrationId,
    event_tx: TxEventChannel,
    config: TuyaConfig,
    device_expected_states: HashMap<DeviceId, Arc<RwLock<Device>>>,
    device_poll_handles: HashMap<DeviceId, JoinHandle<()>>,
}

fn default_device(device_id: DeviceId, name: String, integration_id: IntegrationId) -> Device {
    Device {
        id: device_id,
        name,
        integration_id,
        scene: None,
        state: DeviceState::Light(Light {
            power: false,
            brightness: None,
            color: None,
            cct: None,
            transition_ms: None,
        }),
    }
}

#[async_trait]
impl Integration for Tuya {
    fn new(id: &IntegrationId, config: &config::Value, event_tx: TxEventChannel) -> Result<Self> {
        let config: TuyaConfig = config
            .clone()
            .try_into()
            .context("Failed to deserialize config of Tuya integration")?;

        Ok(Tuya {
            id: id.clone(),
            config,
            event_tx,
            device_expected_states: HashMap::new(),
            device_poll_handles: HashMap::new(),
        })
    }

    async fn register(&mut self) -> Result<()> {
        let mut device_expected_states = HashMap::new();

        let integration_id = self.id.clone();

        for (device_id, device_config) in &self.config.devices {
            let device_id = device_id.clone();
            let device_config = device_config.clone();
            let event_tx = self.event_tx.clone();
            let integration_id = integration_id.clone();

            println!("Getting initial state of {}", device_config.name);
            let device = get_tuya_state(&device_id, &integration_id, &device_config).await;
            let device = device.unwrap_or_else(|_| {
                println!("Failed to get initial state of Tuya device {}, creating Device with default state", device_config.name);

                default_device(device_id.clone(), device_config.name.clone(), integration_id)
            });

            device_expected_states.insert(device_id.clone(), Arc::new(RwLock::new(device.clone())));

            event_tx.send(Message::IntegrationDeviceRefresh { device });
        }

        self.device_expected_states = device_expected_states;

        println!("registered tuya integration {}", self.id);

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        println!("starting tuya integration {}", self.id);

        let device_expected_states = self.device_expected_states.clone();

        for (device_id, device_config) in &self.config.devices {
            let device_config = device_config.clone();
            let device_expected_state = device_expected_states.get(device_id).unwrap().clone();
            let sender = self.event_tx.clone();

            let handle = task::spawn(async move {
                poll_light(&device_config, sender, device_expected_state).await
            });

            self.device_poll_handles.insert(device_id.clone(), handle);
        }

        println!("started tuya integration {}", self.id);

        Ok(())
    }

    async fn set_integration_device_state(&mut self, device: &Device) -> Result<()> {
        {
            let device_expected_state = self.device_expected_states.get(&device.id).unwrap();
            let mut device_expected_state = device_expected_state.write().await;
            *device_expected_state = device.clone();
        }

        let mut device_poll_handle = self.device_poll_handles.remove(&device.id);
        if let Some(poll_handle) = device_poll_handle.take() {
            poll_handle.cancel().await;
        }

        let config = self.config.clone();
        let sender = self.event_tx.clone();

        let device_config = config
            .devices
            .get(&device.id)
            .context(format!(
                "Could not find TuyaDeviceConfig for device with id {}",
                device.id,
            ))?
            .clone();
        let device_expected_state = self.device_expected_states.get(&device.id).unwrap().clone();

        let handle = {
            let device_config = device_config.clone();
            task::spawn(
                async move { poll_light(&device_config, sender, device_expected_state).await },
            )
        };

        self.device_poll_handles.insert(device.id.clone(), handle);

        set_tuya_state(device, &device_config).await?;
        Ok(())
    }

    async fn run_integration_action(&mut self, _: &IntegrationActionPayload) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct TuyaState {
    power_on: bool,
    brightness: u32,
    color_temperature: u32,
}

fn to_tuya_state(device: &Device) -> Result<TuyaState> {
    let light_state = match device.state {
        DeviceState::Light(Light {
            brightness,
            color,
            power,
            transition_ms,
            ref cct,
        }) => Ok(Light {
            power,
            brightness,
            color,
            transition_ms,
            cct: cct.clone(),
        }),
        _ => Err(anyhow!("Unsupported device state")),
    }?;

    let color = light_state.color.unwrap_or_else(|| Hsv::new(0.0, 1.0, 1.0));

    let brightness = light_state.brightness.unwrap_or(1.0) * color.value;

    // Brightness goes from 10 to 1000 ¯\_(ツ)_/¯
    let brightness = f32::floor(brightness * 990.0) as u32 + 10;

    let cct = light_state.cct.map(|cct| cct.get_cct()).unwrap_or(4000.0);

    // Range of my bulbs is from 2700K - 4100K (and they express this as a
    // 0-1000 range), this is very likely not true for all Tuya bulbs
    let min_supported_temp = 2700.0;
    let max_supported_temp = 4100.0;

    // Scale the value into 0.0 - 1.0 range
    let q = (cct - min_supported_temp) / (max_supported_temp - min_supported_temp);
    let q = q.clamp(0.0, 1.0);

    // Scale the value into 0 - 1000 range
    let color_temperature = f32::floor(q * 1000.0) as u32;

    let state = TuyaState {
        power_on: light_state.power,
        brightness,
        color_temperature,
    };

    Ok(state)
}

async fn set_tuya_state(device: &Device, device_config: &TuyaDeviceConfig) -> Result<()> {
    // println!("setting tuya state: {:?} {}", device.state, device.name);
    let tuya_state = to_tuya_state(device)?;

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    {
        // Create a TuyaDevice, this is the type used to set/get status to/from a Tuya compatible
        // device.
        let tuya_device = TuyaDevice::create(
            "3.3",
            Some(&device_config.local_key),
            IpAddr::from_str(&device_config.ip).unwrap(),
        )?;

        let mut dps = HashMap::new();
        dps.insert(
            device_config.power_on_field.clone(),
            json!(tuya_state.power_on),
        );
        dps.insert(
            device_config.brightness_field.clone(),
            json!(tuya_state.brightness),
        );
        dps.insert(
            device_config.color_temp_field.clone(),
            json!(tuya_state.color_temperature),
        );

        // Create the payload to be sent, this will be serialized to the JSON format
        let payload = Payload::Struct(PayloadStruct {
            dev_id: device.id.to_string(),
            // gw_id: None,
            gw_id: Some(device.id.to_string()),
            uid: Some(device.id.to_string()),
            t: Some(current_time),
            dp_id: None,
            dps: Some(dps),
        });

        tokio::time::timeout(Duration::from_millis(250), tuya_device.set(payload, 0)).await??
    }

    Ok(())
}

async fn get_tuya_state(
    device_id: &DeviceId,
    integration_id: &IntegrationId,
    device_config: &TuyaDeviceConfig,
) -> Result<Device> {
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    let response = {
        // Create a TuyaDevice, this is the type used to set/get status to/from a Tuya compatible
        // device.
        let tuya_device = TuyaDevice::create(
            "3.3",
            Some(&device_config.local_key),
            IpAddr::from_str(&device_config.ip).unwrap(),
        )?;

        // Create the payload to be sent, this will be serialized to the JSON format
        let payload = Payload::Struct(PayloadStruct {
            dev_id: device_id.to_string(),
            gw_id: Some(device_id.to_string()),
            uid: Some(device_id.to_string()),
            t: Some(current_time),
            dp_id: None,
            dps: Some(HashMap::new()),
        });

        tokio::time::timeout(Duration::from_millis(250), tuya_device.get(payload, 0))
            .await?
            .context(format!("Error while polling {}", device_config.name))?
    };

    let first = response
        .first()
        .context("Could not find valid Tuya Message in response")?;

    if let Payload::Struct(PayloadStruct { dps: Some(dps), .. }) = &first.payload {
        let power = if let Some(Value::Bool(value)) = dps.get(&device_config.power_on_field) {
            *value
        } else {
            true
        };

        let brightness =
            if let Some(Value::Number(value)) = dps.get(&device_config.brightness_field) {
                // Brightness goes from 10 to 1000 ¯\_(ツ)_/¯
                let bri = value.as_u64().unwrap_or(1000);

                // Scale range to 0-990
                let bri = bri - 10;

                // Scale range to 0-1
                Some(bri as f32 / 990.0)
            } else {
                None
            };

        let cct = if let Some(Value::Number(value)) = dps.get(&device_config.color_temp_field) {
            // Range of my bulbs is from 2700K - 4100K (and they express this as a
            // 0-1000 range), this is very likely not true for all Tuya bulbs
            let min_supported_temp = 2700.0;
            let max_supported_temp = 4100.0;

            let ct = value.as_u64().unwrap_or(1000);

            // Scale range to 0-1
            let q = ct as f32 / 1000.0;

            let cct = q * (max_supported_temp - min_supported_temp) + min_supported_temp;

            Some(CorrelatedColorTemperature::new(
                cct,
                Range {
                    start: min_supported_temp,
                    end: max_supported_temp,
                },
            ))
        } else {
            None
        };

        let state = DeviceState::Light(Light::new_with_cct(power, brightness, cct, Some(1000)));

        let device = Device {
            id: device_id.clone(),
            name: device_config.name.clone(),
            integration_id: integration_id.clone(),
            scene: None,
            state,
        };

        Ok(device)
    } else {
        Err(anyhow!("Unsupported Tuya response"))
    }
}

pub async fn poll_light(
    device_config: &TuyaDeviceConfig,
    sender: TxEventChannel,
    device_expected_state: Arc<RwLock<Device>>,
) {
    let poll_rate = Duration::from_millis(1000);
    let mut interval = stream::interval(poll_rate);

    loop {
        interval.next().await;

        let device_expected_state = { device_expected_state.read().await.clone() };
        let result = set_tuya_state(&device_expected_state, device_config).await;

        if let Err(e) = result {
            eprintln!(
                "Error while polling Tuya state for device {}: {:?}",
                device_expected_state.name, e
            );
        }

        // We still need to send our version of the device state to homectl core, in
        // case it has gone stale.
        sender.send(Message::IntegrationDeviceRefresh {
            device: device_expected_state,
        });
    }
}