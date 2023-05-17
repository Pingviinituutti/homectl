use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    integration::IntegrationActionDescriptor,
    scene::{CycleScenesDescriptor, SceneDescriptor},
    dim::DimDescriptor,
};

#[derive(TS, Clone, Deserialize, Debug, Serialize)]
#[serde(tag = "action")]
#[ts(export)]
pub enum Action {
    /// Request to activate given scene.
    ActivateScene(SceneDescriptor),

    /// Request to cycle between given scenes.
    CycleScenes(CycleScenesDescriptor),

    /// Runs an integration action
    IntegrationAction(IntegrationActionDescriptor),

    /// Dims the given groups and devices
    DimAction(DimDescriptor),
}

pub type Actions = Vec<Action>;
