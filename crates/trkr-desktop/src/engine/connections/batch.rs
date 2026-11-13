//! Best-effort All Connect orchestration, preserving workflow assignments.
use super::*;

impl Session {
    pub(in crate::engine) fn connect_inventory(
        &mut self,
        inventory: crate::inventory::Inventory,
        mut errors: Vec<String>,
        cancel: &AtomicBool,
        send: &SyncSender<Event>,
    ) -> Result<(), String> {
        // Ambiguous identities cannot safely own connections. Device settings,
        // however, are checked per entry so one bad port cannot block the rest.
        inventory.validate_connection_ids()?;
        let lockin = inventory.lockin().ok();
        // A rejected edit must not replace an existing connection's confirmed
        // identity, or accidentally select a different measurement instrument.
        let confirmed_lockin = self
            .config
            .as_ref()
            .filter(|c| c.device_ids.lockin == inventory.selected.lockin)
            .map(|c| (c.model, c.address))
            .or_else(|| {
                self.connections
                    .lockins
                    .get(&inventory.selected.lockin)
                    .map(|l| (l.model, l.address))
            })
            .unwrap_or((Model::Sr830, 0));
        let scanner = inventory.scanner().ok().flatten();
        let selected = HardwareConfig {
            power_meter: inventory
                .power_meter()
                .ok()
                .flatten()
                .map(|p| p.config.clone()),
            device_ids: inventory.selected.clone(),
            model: lockin.map(|l| l.model).unwrap_or(confirmed_lockin.0),
            address: lockin.map(|l| l.address).unwrap_or(confirmed_lockin.1),
            motion: MotionConfig {
                delay: inventory.delay().ok().flatten().map(|d| d.config.clone()),
                x: scanner.and_then(|s| s.x.clone()),
                y: scanner.and_then(|s| s.y.clone()),
            },
        };
        for l in &inventory.lockins {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let mut c = selected.clone();
            c.device_ids.lockin = l.id.clone();
            // Connecting this lock-in must not read an unrelated selected stage.
            c.device_ids.delay.clear();
            c.device_ids.scanner.clear();
            c.motion = MotionConfig::default();
            c.model = l.model;
            c.address = l.address;
            let result = if (1..=30).contains(&l.address) {
                self.connect_registered(c, vec![Device::Lockin])
            } else {
                Err("GPIB address must be 1..30".into())
            };
            if let Err(error) = result {
                errors.push(format!("{} [{}]: {error}", l.display_name(), l.id));
            }
            self.clear_failed_lockin();
        }
        for d in &inventory.delays {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let mut c = selected.clone();
            c.device_ids.delay = d.id.clone();
            c.motion = MotionConfig {
                delay: Some(d.config.clone()),
                ..Default::default()
            };
            let result = if d.zero_ps.is_finite() {
                self.connections.connect_motion(&c, &[Device::T])
            } else {
                Err("Delay zero must be finite".into())
            };
            if let Err(error) = result {
                errors.push(format!("{} [{}]: {error}", d.display_name(), d.id));
            }
        }
        for s in &inventory.scanners {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let mut c = selected.clone();
            c.device_ids.scanner = s.id.clone();
            c.motion = MotionConfig {
                x: s.x.clone(),
                y: s.y.clone(),
                ..Default::default()
            };
            let devices: Vec<_> = [(Device::X, s.x.is_some()), (Device::Y, s.y.is_some())]
                .into_iter()
                .filter_map(|(d, present)| present.then_some(d))
                .collect();
            let result = if !s.zero_um.iter().all(|v| v.is_finite()) {
                Err("Scanner zeros must be finite".into())
            } else if devices.is_empty() {
                Err("No scanner axes configured".into())
            } else {
                self.connections.connect_motion(&c, &devices)
            };
            if let Err(error) = result {
                errors.push(format!("{} [{}]: {error}", s.display_name(), s.id));
            }
        }
        for entry in &inventory.power_meters {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            // An unassigned resource is an optional draft, not a connection.
            let result = entry.config.validate(true).and_then(|_| {
                if entry.config.resource.trim().is_empty() {
                    Ok(())
                } else {
                    self.connect_power(entry.id.clone(), entry.config.clone(), send)
                }
            });
            if let Err(error) = result {
                errors.push(format!("{} [{}]: {error}", entry.display_name(), entry.id));
            }
        }
        if let Err(error) = self.select_config(selected) {
            errors.push(format!("Restore measurement selection: {error}"));
        }
        self.refresh_device_states();
        self.refresh_power_states();
        if cancel.load(Ordering::Acquire) {
            errors.push("Connection attempts stopped".into());
        }
        if errors.is_empty() {
            return Ok(());
        }
        let connected = self
            .state
            .registered_devices
            .iter()
            .map(|s| format!("{} {}", s.kind, s.id))
            .chain(
                self.state
                    .power_meters
                    .iter()
                    .filter(|p| p.connected)
                    .map(|p| format!("power meter {}", p.id)),
            )
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "All Connect incomplete: {}. Connections retained: {}",
            errors.join("; "),
            if connected.is_empty() {
                "none"
            } else {
                &connected
            }
        ))
    }
}
