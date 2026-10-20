// Buttplug Rust Source Code File - See https://buttplug.io for more info.
//
// Copyright 2016-2026 Nonpolynomial Labs LLC. All rights reserved.
//
// Licensed under the BSD 3-Clause license. See LICENSE file in the project root
// for full license information.

use std::sync::Arc;

use async_trait::async_trait;
use buttplug_core::errors::ButtplugDeviceError;
use buttplug_server_device_config::{
  Endpoint,
  ProtocolCommunicationSpecifier,
  ServerDeviceDefinition,
  UserDeviceIdentifier,
};
use uuid::{Uuid, uuid};

use crate::device::{
  hardware::{
    Hardware,
    HardwareCommand,
    HardwareEvent,
    HardwareSubscribeCmd,
    HardwareUnsubscribeCmd,
    HardwareWriteCmd,
  },
  protocol::{
    ProtocolHandler,
    ProtocolIdentifier,
    ProtocolInitializer,
    generic_protocol_initializer_setup,
  },
};

const LELO_INA_THRUST_PROTOCOL_UUID: Uuid = uuid!("0046deb1-f9b0-4e33-b818-2261c9e7ddd3");
const THRUST_CHANNEL: u8 = 0x01;
const VIBRATE_CHANNEL: u8 = 0x02;

generic_protocol_initializer_setup!(LeloInaThrust, "lelo-ina-thrust");

#[derive(Default)]
pub struct LeloInaThrustInitializer {}

impl LeloInaThrustInitializer {
  async fn stop_all_outputs(hardware: &Hardware) -> Result<(), ButtplugDeviceError> {
    for channel in [THRUST_CHANNEL, VIBRATE_CHANNEL] {
      hardware
        .write_value(&HardwareWriteCmd::new(
          &[LELO_INA_THRUST_PROTOCOL_UUID],
          Endpoint::Tx,
          vec![0x0a, 0x12, channel, 0x00],
          true,
        ))
        .await?;
    }
    Ok(())
  }
}

#[async_trait]
impl ProtocolInitializer for LeloInaThrustInitializer {
  async fn initialize(
    &mut self,
    hardware: Arc<Hardware>,
    _: &ServerDeviceDefinition,
  ) -> Result<Arc<dyn ProtocolHandler>, ButtplugDeviceError> {
    let mut event_receiver = hardware.event_stream();
    hardware
      .subscribe(&HardwareSubscribeCmd::new(
        LELO_INA_THRUST_PROTOCOL_UUID,
        Endpoint::Whitelist,
      ))
      .await?;

    loop {
      let event = event_receiver.recv().await;
      if let Ok(HardwareEvent::Notification(_, _, token)) = event {
        if token.len() != 8 {
          return Err(ButtplugDeviceError::ProtocolSpecificError(
            "LeloInaThrust".to_owned(),
            format!(
              "Lelo Ina Thrust returned a malformed {}-byte security handshake",
              token.len()
            ),
          ));
        }

        if token.iter().all(|byte| *byte == 0u8) {
          info!(
            "Lelo Ina Thrust isn't authorised: Tap the device's power button to complete connection."
          );
        } else if token[0] == 1u8 && token[1..].iter().all(|byte| *byte == 0u8) {
          debug!("Lelo Ina Thrust is authorised!");
          // Physical testing showed that Ina can remain active after a GATT disconnect. Ensure the
          // first actuator writes after every successful authorization or reconnect are stops.
          Self::stop_all_outputs(&hardware).await?;
          return Ok(Arc::new(LeloInaThrust::default()));
        } else {
          debug!("Lelo Ina Thrust gave us a password: {:?}", token);
          // The token cannot be written while subscribed to the whitelist endpoint.
          hardware
            .unsubscribe(&HardwareUnsubscribeCmd::new(
              LELO_INA_THRUST_PROTOCOL_UUID,
              Endpoint::Whitelist,
            ))
            .await?;
          hardware
            .write_value(&HardwareWriteCmd::new(
              &[LELO_INA_THRUST_PROTOCOL_UUID],
              Endpoint::Whitelist,
              token,
              true,
            ))
            .await?;
          hardware
            .subscribe(&HardwareSubscribeCmd::new(
              LELO_INA_THRUST_PROTOCOL_UUID,
              Endpoint::Whitelist,
            ))
            .await?;
        }
      } else {
        return Err(ButtplugDeviceError::ProtocolSpecificError(
          "LeloInaThrust".to_owned(),
          "Lelo Ina Thrust didn't provide a valid security handshake".to_owned(),
        ));
      }
    }
  }
}

#[derive(Default)]
pub struct LeloInaThrust {}

impl LeloInaThrust {
  fn handle_scalar_cmd(
    &self,
    feature_index: u32,
    feature_id: Uuid,
    speed: u32,
  ) -> Result<Vec<HardwareCommand>, ButtplugDeviceError> {
    if speed > 100 {
      return Err(ButtplugDeviceError::ProtocolSpecificError(
        "LeloInaThrust".to_owned(),
        format!("Lelo Ina Thrust speed {speed} exceeds the protocol maximum of 100"),
      ));
    }

    let channel = feature_index as u8 + 1;
    let data = if speed == 0 {
      vec![0x0a, 0x12, channel, 0x00]
    } else {
      // Both 0x03e8 fields are required by physical testing. Their semantics are unknown.
      vec![
        0x0a,
        0x12,
        channel,
        0x08,
        0x03,
        0xe8,
        0x03,
        0xe8,
        speed as u8,
        0x00,
      ]
    };

    Ok(vec![
      HardwareWriteCmd::new(&[feature_id], Endpoint::Tx, data, true).into(),
    ])
  }
}

impl ProtocolHandler for LeloInaThrust {
  fn handle_output_oscillate_cmd(
    &self,
    feature_index: u32,
    feature_id: Uuid,
    speed: u32,
  ) -> Result<Vec<HardwareCommand>, ButtplugDeviceError> {
    self.handle_scalar_cmd(feature_index, feature_id, speed)
  }

  fn handle_output_vibrate_cmd(
    &self,
    feature_index: u32,
    feature_id: Uuid,
    speed: u32,
  ) -> Result<Vec<HardwareCommand>, ButtplugDeviceError> {
    self.handle_scalar_cmd(feature_index, feature_id, speed)
  }
}
