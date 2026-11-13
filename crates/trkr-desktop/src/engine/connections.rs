//! Persistent instrument connections; selecting a measurement never reopens a port.
use super::*;
mod batch;
use std::collections::BTreeMap;
use trkr_motion::MotionPortPool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionKind {
    Delay,
    Scanner,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceState {
    pub id: String,
    pub kind: String,
    pub connected: [bool; 3],
    pub position: Position,
    pub delay_calibration: Option<DelayCalibration>,
    pub phase: String,
}

pub(super) struct ConnectedMotion {
    pub config: MotionConfig,
    pub rig: MotionRig,
    pub state: DeviceState,
}
pub(super) struct ParkedLockin {
    gpib: Gpib,
    model: Model,
    address: u8,
    mode: DisplayMode,
}
#[derive(Default)]
pub(super) struct Connections {
    pool: MotionPortPool,
    gpib_pool: trkr_transport::GpibPool,
    pub motions: BTreeMap<(MotionKind, String), ConnectedMotion>,
    lockins: BTreeMap<String, ParkedLockin>,
}

struct SelectionView {
    rig: Option<MotionRig>,
    mask: [bool; 3],
    position: Position,
    calibration: Option<DelayCalibration>,
}
impl Connections {
    pub(super) fn clear_failed_lockins(&mut self) -> bool {
        let before = self.lockins.len();
        self.lockins.retain(|_, l| !l.gpib.is_poisoned());
        before != self.lockins.len()
    }
    fn selected_view(&self, config: &HardwareConfig) -> Result<SelectionView, String> {
        let delay = self
            .motions
            .get(&(MotionKind::Delay, config.device_ids.delay.clone()));
        let scanner = self
            .motions
            .get(&(MotionKind::Scanner, config.device_ids.scanner.clone()));
        let rig = if delay.is_some() || scanner.is_some() {
            Some(MotionRig::selected_view(
                delay.map(|e| &e.rig),
                scanner.map(|e| &e.rig),
            )?)
        } else {
            None
        };
        let mut position = Position::default();
        if let Some(d) = delay {
            position.physical[0] = d.state.position.physical[0];
            position.delay_stage_mm = d.state.position.delay_stage_mm;
            position.delay_stage_pulse = d.state.position.delay_stage_pulse;
        }
        if let Some(s) = scanner {
            position.physical[1..].copy_from_slice(&s.state.position.physical[1..]);
            position.x_scanner_mm = s.state.position.x_scanner_mm;
            position.x_scanner_deg = s.state.position.x_scanner_deg;
            position.y_scanner_mm = s.state.position.y_scanner_mm;
            position.y_scanner_deg = s.state.position.y_scanner_deg;
        }
        Ok(SelectionView {
            rig,
            mask: [
                delay.is_some_and(|d| d.state.connected[0]),
                scanner.is_some_and(|s| s.state.connected[1]),
                scanner.is_some_and(|s| s.state.connected[2]),
            ],
            position,
            calibration: delay.and_then(|d| d.state.delay_calibration.clone()),
        })
    }
    fn connect_motion(
        &mut self,
        config: &HardwareConfig,
        devices: &[Device],
    ) -> Result<(), String> {
        for kind in [MotionKind::Delay, MotionKind::Scanner] {
            let requested = match kind {
                MotionKind::Delay => [devices.contains(&Device::T), false, false],
                MotionKind::Scanner => [
                    false,
                    devices.contains(&Device::X),
                    devices.contains(&Device::Y),
                ],
            };
            if !requested.into_iter().any(|v| v) {
                continue;
            }
            let id = match kind {
                MotionKind::Delay => &config.device_ids.delay,
                MotionKind::Scanner => &config.device_ids.scanner,
            };
            let key = (kind, id.clone());
            let previous = self.motions.get(&key);
            let mask = std::array::from_fn::<_, 3, _>(|i| {
                requested[i] || previous.is_some_and(|p| p.state.connected[i])
            });
            let motion = MotionConfig {
                delay: mask[0].then(|| config.motion.delay.clone()).flatten(),
                x: mask[1].then(|| config.motion.x.clone()).flatten(),
                y: mask[2].then(|| config.motion.y.clone()).flatten(),
            };
            if mask
                != [
                    motion.delay.is_some(),
                    motion.x.is_some(),
                    motion.y.is_some(),
                ]
            {
                return Err("No configuration for the requested motion device".into());
            }
            if previous.is_some_and(|p| p.config == motion) {
                continue;
            }
            validate_motion_identity(&key, &motion, &self.motions)?;
            // The old connection remains leased until the replacement opens.
            // Changes to bus settings require an explicit disconnect first.
            let mut rig = MotionRig::open_with_pool(&motion, &mut self.pool)?;
            let state = DeviceState {
                id: id.clone(),
                kind: match kind {
                    MotionKind::Delay => "delay",
                    MotionKind::Scanner => "scanner",
                }
                .into(),
                connected: mask,
                position: rig.position()?,
                delay_calibration: rig.delay_calibration()?,
                phase: "Ready".into(),
            };
            self.motions.insert(
                key,
                ConnectedMotion {
                    config: motion,
                    rig,
                    state,
                },
            );
        }
        Ok(())
    }
}

fn endpoints(config: &MotionConfig) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    if let Some(c) = &config.delay {
        let c = c.normalized()?;
        out.push((
            c.port.trim().to_ascii_uppercase(),
            format!("delay:{}:{}", c.controller, c.controller_axis),
        ));
    }
    for c in [&config.x, &config.y].into_iter().flatten() {
        let c = c.normalized()?;
        out.push((
            c.port.trim().to_ascii_uppercase(),
            format!(
                "scanner:{}:{}:{}",
                c.controller,
                c.controller_address,
                if c.controller == "CONEXCC" {
                    "1"
                } else {
                    &c.axis
                }
            ),
        ));
    }
    Ok(out)
}
fn validate_motion_identity(
    key: &(MotionKind, String),
    config: &MotionConfig,
    connected: &BTreeMap<(MotionKind, String), ConnectedMotion>,
) -> Result<(), String> {
    let requested = endpoints(config)?;
    for (other_key, other) in connected {
        if other_key == key {
            continue;
        }
        let occupied = endpoints(&other.config)?;
        if requested.iter().any(|v| occupied.contains(v)) {
            return Err(format!(
                "A physical axis is already connected as {}",
                other_key.1
            ));
        }
    }
    Ok(())
}

fn disconnect_axes(kind: MotionKind, devices: &[Device]) -> [bool; 3] {
    match kind {
        MotionKind::Delay => [devices.contains(&Device::T), false, false],
        MotionKind::Scanner => [
            false,
            devices.contains(&Device::X),
            devices.contains(&Device::Y),
        ],
    }
}

impl Session {
    pub(super) fn disconnect_all(&mut self) -> Result<(), String> {
        self.power_meters.clear();
        self.refresh_power_states();
        self.rig = None;
        self.gpib = None;
        self.connections = Connections::default();
        self.state.devices = [false; 4];
        self.state.position = Position::default();
        self.state.signal = None;
        self.state.signal_origin = None;
        self.state.lockin = Value::Null;
        self.lockin_context = None;
        self.state.transport.clear();
        self.state.delay_calibration = None;
        self.refresh_device_states();
        Ok(())
    }
    pub(super) fn reverse_direction(
        &mut self,
        kind: MotionKind,
        id: String,
        axis: usize,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        if !matches!(
            (kind, axis),
            (MotionKind::Delay, 0) | (MotionKind::Scanner, 1..=2)
        ) {
            return Err("Axis does not belong to this device".into());
        }
        let key = (kind, id.clone());
        let entry = self
            .connections
            .motions
            .get_mut(&key)
            .ok_or("Connect this device before reversing Direction")?;
        let position = entry.rig.position()?;
        let mut config = entry.rig.recording_configuration();
        if axis == 0 {
            let micro = entry
                .rig
                .delay_calibration()?
                .and_then(|c| c.microstep_division)
                .ok_or("No observed delay calibration")?;
            config.delay = Some(
                config
                    .delay
                    .as_ref()
                    .ok_or("No delay stage")?
                    .reversed_preserving_display_at_mm_with_micro(
                        position.delay_stage_mm.ok_or("No delay readback")?,
                        micro,
                    )?,
            );
        } else {
            let raw = if axis == 1 {
                position.x_scanner_mm.or(position.x_scanner_deg)
            } else {
                position.y_scanner_mm.or(position.y_scanner_deg)
            }
            .ok_or("No scanner readback")?;
            let scanner = if axis == 1 {
                &mut config.x
            } else {
                &mut config.y
            };
            *scanner = Some(
                scanner
                    .as_ref()
                    .ok_or("No scanner axis")?
                    .reversed_preserving_display_at_raw(raw)?,
            );
        }
        let mut rig = MotionRig::open_with_pool(&config, &mut self.connections.pool)?;
        let position = rig.position()?;
        let calibration = rig.delay_calibration()?;
        let mut replacement = None;
        if let Some(active) = &self.config {
            let selected = (kind == MotionKind::Delay && active.device_ids.delay == id)
                || (kind == MotionKind::Scanner && active.device_ids.scanner == id);
            if selected {
                let delay = if kind == MotionKind::Delay {
                    Some(&rig)
                } else {
                    self.connections
                        .motions
                        .get(&(MotionKind::Delay, active.device_ids.delay.clone()))
                        .map(|e| &e.rig)
                };
                let scanner = if kind == MotionKind::Scanner {
                    Some(&rig)
                } else {
                    self.connections
                        .motions
                        .get(&(MotionKind::Scanner, active.device_ids.scanner.clone()))
                        .map(|e| &e.rig)
                };
                let mut view = MotionRig::selected_view(delay, scanner)?;
                let readback = view.position()?;
                let scale = view.delay_calibration()?;
                replacement = Some((view, readback, scale));
            }
        }
        // Commit only after every required readback and view has succeeded.
        let entry = self
            .connections
            .motions
            .get_mut(&key)
            .expect("existing device");
        entry.config = config.clone();
        entry.rig = rig;
        entry.state.position = position;
        entry.state.delay_calibration = calibration;
        if let Some((view, position, scale)) = replacement {
            self.rig = Some(view);
            self.state.position = position;
            self.state.delay_calibration = scale;
            let active = self.config.as_mut().expect("selected config");
            if kind == MotionKind::Delay {
                active.motion.delay = config.delay.clone();
            } else {
                active.motion.x = config.x.clone();
                active.motion.y = config.y.clone();
            }
        }
        self.refresh_device_states();
        let _ = send.send(Event::DirectionChanged {
            kind,
            id,
            config: Box::new(config),
        });
        Ok(())
    }
    pub(super) fn sync_selected_readbacks(&mut self) {
        if let Some(config) = &self.config {
            for (kind, id) in [
                (MotionKind::Delay, &config.device_ids.delay),
                (MotionKind::Scanner, &config.device_ids.scanner),
            ] {
                if let Some(entry) = self.connections.motions.get_mut(&(kind, id.clone())) {
                    let source = &self.state.position;
                    if kind == MotionKind::Delay {
                        entry.state.position.physical[0] = source.physical[0];
                        entry.state.position.delay_stage_mm = source.delay_stage_mm;
                        entry.state.position.delay_stage_pulse = source.delay_stage_pulse;
                    } else {
                        entry.state.position.physical[1..].copy_from_slice(&source.physical[1..]);
                        entry.state.position.x_scanner_mm = source.x_scanner_mm;
                        entry.state.position.x_scanner_deg = source.x_scanner_deg;
                        entry.state.position.y_scanner_mm = source.y_scanner_mm;
                        entry.state.position.y_scanner_deg = source.y_scanner_deg;
                    }
                }
            }
        }
        self.refresh_device_states();
    }
    pub(super) fn select_config(&mut self, config: HardwareConfig) -> Result<(), String> {
        if let Some(old) = &self.config
            && self.gpib.is_some()
            && old.device_ids.lockin == config.device_ids.lockin
            && (old.model != config.model || old.address != config.address)
        {
            return Err("Disconnect the lock-in before changing its model or address".into());
        }
        if let Some(parked) = self.connections.lockins.get(&config.device_ids.lockin)
            && (parked.model != config.model || parked.address != config.address)
        {
            return Err("Disconnect the lock-in before changing its model or address".into());
        }
        // Prepare the complete selection before mutating connection ownership.
        // Selecting uses the last confirmed readback; polling refreshes it without reopening.
        let view = self.connections.selected_view(&config)?;
        let switch_lockin = self.config.as_ref().is_some_and(|old| {
            old.device_ids.lockin != config.device_ids.lockin
                || old.model != config.model
                || old.address != config.address
        });
        if switch_lockin {
            if let (Some(old), Some(gpib)) = (&self.config, self.gpib.take())
                && old.device_ids.lockin != config.device_ids.lockin
            {
                self.connections.lockins.insert(
                    old.device_ids.lockin.clone(),
                    ParkedLockin {
                        gpib,
                        model: old.model,
                        address: old.address,
                        mode: self.state.signal_mode,
                    },
                );
            }
            self.state.signal = None;
            self.state.signal_origin = None;
            self.state.lockin = Value::Null;
            self.lockin_context = None;
        }
        if self.gpib.is_none()
            && let Some(parked) = self.connections.lockins.remove(&config.device_ids.lockin)
        {
            self.state.signal_mode = parked.mode;
            self.gpib = Some(parked.gpib);
        }
        self.config = Some(config);
        self.install_selection_view(view);
        self.state.devices[0] = self.gpib.is_some();
        self.state.transport = self
            .gpib
            .as_ref()
            .map(|g| g.backend_name().to_owned())
            .unwrap_or_default();
        self.refresh_device_states();
        Ok(())
    }
    fn install_selection_view(&mut self, view: SelectionView) {
        self.rig = view.rig;
        self.state.devices[1..].copy_from_slice(&view.mask);
        self.state.position = view.position;
        self.state.delay_calibration = view.calibration;
    }
    pub(super) fn rebuild_selected_motion(&mut self) -> Result<(), String> {
        let Some(config) = &self.config else {
            return Ok(());
        };
        let view = self.connections.selected_view(config)?;
        self.install_selection_view(view);
        Ok(())
    }
    pub(super) fn connect_registered(
        &mut self,
        config: HardwareConfig,
        devices: Vec<Device>,
    ) -> Result<(), String> {
        self.select_config(config.clone())?;
        if devices.contains(&Device::Lockin) && self.gpib.is_none() {
            if self
                .connections
                .lockins
                .values()
                .any(|l| l.address == config.address)
            {
                return Err("This GPIB address is already connected as another lock-in".into());
            }
            self.gpib = Some(self.connections.gpib_pool.connect(config.address)?);
        }
        self.connections.connect_motion(&config, &devices)?;
        self.rebuild_selected_motion()?;
        self.state.devices[0] = self.gpib.is_some();
        self.state.transport = self
            .gpib
            .as_ref()
            .map(|g| g.backend_name().to_owned())
            .unwrap_or_default();
        if config.model == Model::Li5640
            && let Some(gpib) = &mut self.gpib
        {
            lockin::select_mode(config.model, gpib, self.state.signal_mode)?;
        }
        self.read()?;
        self.refresh_device_states();
        Ok(())
    }
    pub(super) fn disconnect_registered(&mut self, devices: Vec<Device>) -> Result<(), String> {
        let Some(config) = self.config.clone() else {
            return Ok(());
        };
        if devices.contains(&Device::Lockin) {
            self.gpib = None;
            self.state.devices[0] = false;
            self.state.signal = None;
            self.state.signal_origin = None;
            self.state.lockin = Value::Null;
            self.lockin_context = None;
            self.state.transport.clear();
        }
        for (kind, id) in [
            (MotionKind::Delay, &config.device_ids.delay),
            (MotionKind::Scanner, &config.device_ids.scanner),
        ] {
            let requested = disconnect_axes(kind, &devices);
            // A lock-in-only disconnect must not reopen serial ports or replace
            // their registered rigs. Apart from needless I/O, doing so could
            // lose a healthy motion entry if reopening failed.
            if !requested.iter().any(|requested| *requested) {
                continue;
            }
            let key = (kind, id.clone());
            let Some(entry) = self.connections.motions.get(&key) else {
                continue;
            };
            let prior_config = entry.config.clone();
            let prior_state = entry.state.clone();
            let connected =
                std::array::from_fn(|axis| prior_state.connected[axis] && !requested[axis]);
            if connected.iter().any(|connected| *connected) {
                let remaining = filtered(
                    &prior_config,
                    &[false, connected[0], connected[1], connected[2]],
                );
                // Open and read the replacement while the original entry is
                // still registered. Only a fully ready candidate replaces it.
                let mut rig = MotionRig::open_with_pool(&remaining, &mut self.connections.pool)?;
                let position = rig.position()?;
                let delay_calibration = rig.delay_calibration()?;
                let mut state = prior_state;
                state.connected = connected;
                state.position = position;
                state.delay_calibration = delay_calibration;
                state.phase = "Ready".into();
                self.connections.motions.insert(
                    key,
                    ConnectedMotion {
                        config: remaining,
                        rig,
                        state,
                    },
                );
            } else {
                // Drop the selected view before removing its last registered
                // connection, so a complete disconnect can release the port.
                self.rig = None;
                self.connections.motions.remove(&key);
            }
        }
        self.rebuild_selected_motion()?;
        self.refresh_device_states();
        Ok(())
    }
    pub(super) fn refresh_device_states(&mut self) {
        self.state.registered_devices = self
            .connections
            .motions
            .values()
            .map(|c| c.state.clone())
            .collect();
        let mut lockins: Vec<_> = self.connections.lockins.keys().cloned().collect();
        if let Some(config) = &self.config
            && self.gpib.is_some()
        {
            lockins.push(config.device_ids.lockin.clone());
        }
        for id in lockins {
            self.state.registered_devices.push(DeviceState {
                id,
                kind: "lockin".into(),
                connected: [true, false, false],
                position: Position::default(),
                delay_calibration: None,
                phase: "Ready".into(),
            });
        }
    }
    pub(super) fn poll_registered_positions(&mut self) -> Result<(), String> {
        // One inactive connection per tick bounds UI latency as the inventory grows.
        let count = self.connections.motions.len();
        if count != 0 {
            self.connection_poll_index = (self.connection_poll_index + 1) % count;
            let entry = self
                .connections
                .motions
                .values_mut()
                .nth(self.connection_poll_index)
                .expect("index bounded by connection count");
            match entry.rig.position() {
                Ok(position) => {
                    entry.state.position = position;
                    entry.state.phase = "Ready".into();
                }
                Err(error) => {
                    entry.state.phase = format!("Error: {error}");
                    self.refresh_device_states();
                    return Err(error);
                }
            }
        }
        self.refresh_device_states();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockin_disconnect_does_not_request_motion_replacement() {
        let requested = [Device::Lockin];
        assert_eq!(disconnect_axes(MotionKind::Delay, &requested), [false; 3]);
        assert_eq!(disconnect_axes(MotionKind::Scanner, &requested), [false; 3]);
    }

    #[test]
    fn partial_scanner_disconnect_only_changes_its_requested_axis() {
        assert_eq!(
            disconnect_axes(MotionKind::Scanner, &[Device::X]),
            [false, true, false]
        );
        assert_eq!(
            disconnect_axes(MotionKind::Scanner, &[Device::Y]),
            [false, false, true]
        );
        assert_eq!(
            disconnect_axes(MotionKind::Delay, &[Device::T]),
            [true, false, false]
        );
    }
}
