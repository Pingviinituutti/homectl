use super::{
    device::{Device, DeviceKind},
    events::{Message, TxEventChannel},
};
use std::collections::HashMap;

type State = HashMap<String, Device>;

pub struct DevicesManager {
    sender: TxEventChannel,
    state: State,
}

impl DevicesManager {
    pub fn new(sender: TxEventChannel) -> Self {
        DevicesManager {
            sender,
            state: HashMap::new(),
        }
    }

    /// Checks whether device values were changed or not due to refresh
    pub fn handle_device_refresh(&mut self, device: Device) {
        // println!("handle_device_update for device {}", device.id);

        // FIXME: some of these .clone() calls may be unnecessary?

        let prev_state = self.state.clone();
        let internal_state = prev_state.get(&device.id);

        self.state.insert(device.id.clone(), device.clone());

        // Take action if the device state has changed from stored state
        if internal_state != Some(&device.clone()) {
            let kind = device.kind.clone();

            match (kind, internal_state) {
                // Device was seen for the first time
                (_, None) => {
                    println!("Discovered device: {:?}", device);
                    self.sender
                        .send(Message::DeviceUpdate {
                            old: None,
                            new: device,
                        })
                        .unwrap();
                }

                // Sensor state has changed, defer handling of this update to
                // other subsystems
                (DeviceKind::Sensor(_), Some(old)) => {
                    self.sender
                        .send(Message::DeviceUpdate {
                            old: Some(old.clone()),
                            new: device,
                        })
                        .unwrap();
                }

                // Device state does not match expected state, maybe the device
                // missed a state update or forgot its state? Try fixing this by
                // emitting SetDeviceState message
                (_, Some(expected_state)) => {
                    self.sender
                        .send(Message::SetDeviceState {
                            device: expected_state.clone(),
                        })
                        .unwrap();
                }
            }
        }
    }

    /// Adjusts stored state for given device
    pub fn set_device_state(&mut self, device: Device) {
        self.state.insert(device.id.clone(), device.clone());
    }
}
