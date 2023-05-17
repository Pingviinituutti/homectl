use crate::{device::{CorrelatedColorTemperature, DeviceKey, DeviceState}, scene::ColorConfig};

use super::{
    device::{DeviceColor, DeviceId},
    group::GroupId,
    integration::IntegrationId,
};
use palette::{Hsv};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

pub fn color_config_as_device_color(color_config: ColorConfig) -> DeviceColor {
    DeviceColor::Hsv(match color_config {
        ColorConfig::Lch(lch) => lch.into(),
        ColorConfig::Hsv(hsv) => hsv,
        ColorConfig::Rgb(rgb) => rgb.into(),
    })
}

#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[ts(export)]
pub struct DimDeviceLink {
    pub integration_id: IntegrationId,
    pub device_id: Option<DeviceId>,
    pub name: Option<String>,
    pub brightness: Option<f32>, // allow overriding brightness
}

#[derive(TS, Clone, Deserialize, Serialize, Debug)]
#[ts(export)]
pub struct DimDescriptor {
    /// Optionally only apply scene to these devices
    pub device_keys: Option<Vec<DeviceKey>>,

    /// Optionally only apply scene to these groups
    pub group_keys: Option<Vec<GroupId>>,

    // The amount to dim
    pub step: Option<f32>,
}

#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[ts(export)]
pub struct DimDeviceState {
    pub power: bool,
    #[ts(type = "String")]
    pub color: Option<ColorConfig>,
    pub brightness: Option<f32>,
    pub cct: Option<CorrelatedColorTemperature>,
    pub transition_ms: Option<u64>,
}

#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[serde(untagged)]
#[ts(export)]
pub enum DimDeviceConfig {
    /// Link to another device, means the dim should read current state from
    /// another device
    DimDeviceLink(DimDeviceLink),

    /// Link to another dim, means the dim should merge all state from another
    /// dim
    DimLink(DimDescriptor),

    /// State to be applied to a device
    DimDeviceState(DimDeviceState),
}

pub type DimDevicesConfig = HashMap<IntegrationId, HashMap<DeviceId, DimDeviceConfig>>;

#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[ts(export)]
pub struct DimGroupsConfig (pub HashMap<GroupId, DimDeviceConfig>);

/// Device "search" config as used directly in the configuration file. We use device names instead of device id as key.
#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[ts(export)]
pub struct DimDevicesSearchConfig (pub HashMap<IntegrationId, HashMap<String, DimDeviceConfig>>);

#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[ts(export)]
pub struct DimConfig {
    pub name: String,
    pub devices: Option<DimDevicesSearchConfig>,
    pub groups: Option<DimGroupsConfig>,
    pub hidden: Option<bool>,
}

// pub type DimsConfig = HashMap<SceneId, DimConfig>;

#[derive(TS, Clone, Deserialize, Serialize, Debug, PartialEq)]
#[ts(export)]
pub struct DimDeviceStates(pub HashMap<DeviceKey, DeviceState>);

#[derive(TS, Clone, Deserialize, Debug, Serialize, PartialEq)]
#[ts(export)]
pub struct FlattenedDimConfig {
    pub name: String,
    pub devices: DimDeviceStates,
    pub hidden: Option<bool>,
}

// #[derive(TS, Clone, Deserialize, Serialize, Debug, PartialEq, Default)]
// #[ts(export)]
// pub struct FlattenedDimsConfig(pub HashMap<SceneId, FlattenedDimConfig>);
