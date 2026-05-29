//! Append-only runtime state audit logging.
//!
//! The state actor is the only writer for `AppState`, so this module can
//! record origin device states at startup and then log per-device updates by
//! diffing the published snapshots around each command.

use super::get_db_connection;
use super::schema::StateDeviceEvents;
use crate::core::snapshot::RuntimeSnapshot;
use crate::types::device::{Device, SensorDevice};
use color_eyre::Result;
use sea_orm::sea_query::Query;
use sea_orm::{ConnectionTrait, Statement, StatementBuilder};

fn statement<C, S>(db: &C, builder: S) -> Statement
where
    C: ConnectionTrait,
    S: StatementBuilder,
{
    db.get_database_backend().build(&builder)
}

fn device_kind(device: &Device) -> &'static str {
    if device.is_sensor() {
        "sensor"
    } else {
        "device"
    }
}

fn sensor_state_numeric_value(sensor: &SensorDevice) -> Option<f64> {
    match sensor {
        SensorDevice::Boolean { value } => Some(if *value { 1.0 } else { 0.0 }),
        SensorDevice::Number { value } => Some(*value),
        SensorDevice::Text { value } => value.parse::<f64>().ok(),
        SensorDevice::Color(_) => None,
    }
}

fn sensor_numeric_value(device: &Device) -> Option<f64> {
    device.get_sensor_state().and_then(sensor_state_numeric_value)
}

async fn insert_device_event_row(event_kind: &str, device: &Device) -> Result<()> {
    let db = get_db_connection()?;
    let device_state_json = device.get_value().to_string();
    let value = sensor_numeric_value(device);

    db.execute(statement(
        db,
        Query::insert()
            .into_table(StateDeviceEvents::Table)
            .columns([
                StateDeviceEvents::DeviceKey,
                StateDeviceEvents::IntegrationId,
                StateDeviceEvents::DeviceId,
                StateDeviceEvents::DeviceName,
                StateDeviceEvents::DeviceKind,
                StateDeviceEvents::EventKind,
                StateDeviceEvents::DeviceStateJson,
                StateDeviceEvents::Value,
            ])
            .values_panic([
                device.get_device_key().to_string().into(),
                device.integration_id.to_string().into(),
                device.id.to_string().into(),
                device.name.clone().into(),
                device_kind(device).into(),
                event_kind.into(),
                device_state_json.into(),
                value.into(),
            ])
            .to_owned(),
    ))
    .await?;

    Ok(())
}

/// Record the initial device list as origin rows.
pub async fn record_origin_device_states(snapshot: &RuntimeSnapshot) -> Result<()> {
    for device in snapshot.devices.0.values() {
        insert_device_event_row("origin", device).await?;
    }

    Ok(())
}

/// Record device-level updates by diffing the previous and current snapshots.
///
/// Subsequent writes are append-only device events so individual sensor/device
/// changes are easy to query.
pub async fn record_device_state_changes(
    previous: &RuntimeSnapshot,
    current: &RuntimeSnapshot,
) -> Result<()> {
    let previous_devices = &previous.devices.0;
    let current_devices = &current.devices.0;

    for (device_key, current_device) in current_devices {
        match previous_devices.get(device_key) {
            Some(previous_device) if current_device.is_state_eq(previous_device) => {}
            _ => insert_device_event_row("upsert", current_device).await?,
        }
    }

    for (device_key, previous_device) in previous_devices {
        if !current_devices.contains_key(device_key) {
            insert_device_event_row("removed", previous_device).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sensor_state_numeric_value;
    use crate::types::device::SensorDevice;

    #[test]
    fn parses_boolean_sensor_to_numeric_value() {
        assert_eq!(
            sensor_state_numeric_value(&SensorDevice::Boolean { value: false }),
            Some(0.0)
        );
        assert_eq!(
            sensor_state_numeric_value(&SensorDevice::Boolean { value: true }),
            Some(1.0)
        );
    }

    #[test]
    fn parses_number_sensor_to_float_value() {
        assert_eq!(
            sensor_state_numeric_value(&SensorDevice::Number { value: 21.5 }),
            Some(21.5)
        );
    }

    #[test]
    fn parses_numeric_text_sensor_to_float_value() {
        assert_eq!(
            sensor_state_numeric_value(&SensorDevice::Text {
                value: "42".to_string(),
            }),
            Some(42.0)
        );

        assert_eq!(
            sensor_state_numeric_value(&SensorDevice::Text {
                value: "21.75".to_string(),
            }),
            Some(21.75)
        );
    }
}
