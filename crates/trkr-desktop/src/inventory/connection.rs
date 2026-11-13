//! Connection-only snapshots: isolate invalid device edits without changing saved profiles.
//! Save/start validation remains in the parent inventory module.
use super::*;

impl Inventory {
    /// Validate the immutable inventory structure used for a best-effort
    /// connection request. Device configuration is deliberately excluded:
    /// one malformed device must not prevent another device from connecting.
    pub fn validate_connection_ids(&self) -> Result<(), String> {
        unique(self.lockins.iter().map(|entry| &entry.id), "lock-in")?;
        unique(self.delays.iter().map(|entry| &entry.id), "delay")?;
        unique(self.scanners.iter().map(|entry| &entry.id), "scanner")?;
        unique(
            self.power_meters.iter().map(|entry| &entry.id),
            "power meter",
        )
    }

    /// Create a connection-only snapshot. It incorporates the current legacy
    /// editor projection for the selected devices, then omits invalid entries
    /// one by one. This never mutates the saved inventory, so a failed edit
    /// cannot accidentally reconnect with a stale configuration.
    pub fn connection_snapshot(&self, raw: &Value) -> Result<(Self, Vec<String>), String> {
        self.validate_connection_ids()?;
        let mut snapshot = self.clone();
        let mut warnings = Vec::new();
        snapshot.apply_selected_legacy_for_connection(raw, &mut warnings);
        snapshot.remove_invalid_connection_entries(&mut warnings);
        Ok((snapshot, warnings))
    }

    /// Promote the first legacy editor device into a connection-only snapshot.
    /// The caller owns the clone, so All Connect never changes the saved
    /// inventory merely by trying a device. IDs are deterministic and avoid
    /// colliding with already registered entries.
    pub fn promote_legacy_for_connection(
        &mut self,
        raw: &Value,
        include_delay: bool,
        include_scanner: bool,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        if include_delay && self.selected.delay.is_empty() {
            let id = unused_id(self.delays.iter().map(|entry| &entry.id), "delay");
            let parsed = raw
                .pointer("/instruments/delay_stage/t")
                .cloned()
                .ok_or_else(|| "Delay-stage configuration is missing".to_owned())
                .and_then(|value| {
                    serde_json::from_value(value.clone())
                        .map_err(|error| format!("Delay-stage configuration: {error}"))
                        .and_then(|config| {
                            number(raw.pointer("/measurements/move_abs/zero"), "t_ps")
                                .map(|zero| (config, zero, value))
                        })
                });
            match parsed {
                Ok((config, zero_ps, raw_config)) => {
                    let mut entry = DelayEntry::new(&id, "Delay 1", config, zero_ps);
                    entry.raw_config = Some(raw_config);
                    self.delays.push(entry);
                    self.selected.delay = id;
                }
                Err(error) => warnings.push(format!("Delay {id} skipped: {error}")),
            }
        }
        if include_scanner && self.selected.scanner.is_empty() {
            let id = unused_id(self.scanners.iter().map(|entry| &entry.id), "scanner");
            let scanner = raw.pointer("/instruments/scanner");
            let x = scanner.map(|value| parse_scanner(value.get("x"), "X"));
            let y = scanner.map(|value| parse_scanner(value.get("y"), "Y"));
            let x = match x {
                Some(Ok(config)) => config,
                Some(Err(error)) => {
                    warnings.push(format!("Scanner {id} X skipped: {error}"));
                    None
                }
                None => None,
            };
            let y = match y {
                Some(Ok(config)) => config,
                Some(Err(error)) => {
                    warnings.push(format!("Scanner {id} Y skipped: {error}"));
                    None
                }
                None => None,
            };
            match (
                number(raw.pointer("/measurements/move_abs/zero"), "x_um"),
                number(raw.pointer("/measurements/move_abs/zero"), "y_um"),
            ) {
                (Ok(x_zero), Ok(y_zero)) if x.is_some() || y.is_some() => {
                    let mut entry = ScannerEntry::new(&id, "Scanner 1", x, y, [x_zero, y_zero]);
                    entry.raw_x = scanner.and_then(|value| value.get("x")).cloned();
                    entry.raw_y = scanner.and_then(|value| value.get("y")).cloned();
                    self.scanners.push(entry);
                    self.selected.scanner = id;
                }
                (x_zero, y_zero) => {
                    let error = x_zero
                        .err()
                        .or_else(|| y_zero.err())
                        .unwrap_or_else(|| "No scanner axis configured".into());
                    warnings.push(format!("Scanner {id} skipped: {error}"));
                }
            }
        }
        warnings
    }

    fn apply_selected_legacy_for_connection(&mut self, raw: &Value, warnings: &mut Vec<String>) {
        let selected_lockin = self.selected.lockin.clone();
        if !selected_lockin.is_empty()
            && let Some(value) = raw.pointer("/instruments/lockin/main")
        {
            match model_from_value(value)
                .and_then(|model| gpib_address(value).map(|address| (model, address)))
            {
                Ok((model, address)) => {
                    if let Some(entry) = self.lockins.iter_mut().find(|e| e.id == selected_lockin) {
                        entry.model = model;
                        entry.address = address;
                        entry.settings = value.clone();
                    }
                }
                Err(error) => {
                    self.lockins.retain(|entry| entry.id != selected_lockin);
                    warnings.push(format!("Lock-in {selected_lockin} skipped: {error}"));
                }
            }
        }

        let selected_delay = self.selected.delay.clone();
        if !selected_delay.is_empty()
            && let Some(value) = raw.pointer("/instruments/delay_stage/t")
        {
            let parsed = serde_json::from_value(value.clone())
                .map_err(|error| format!("Delay-stage configuration: {error}"))
                .and_then(|config| {
                    number(raw.pointer("/measurements/move_abs/zero"), "t_ps")
                        .map(|zero| (config, zero))
                });
            match parsed {
                Ok((config, zero_ps)) => {
                    if let Some(entry) = self.delays.iter_mut().find(|e| e.id == selected_delay) {
                        entry.config = config;
                        entry.zero_ps = zero_ps;
                        entry.raw_config = Some(value.clone());
                    }
                }
                Err(error) => {
                    self.delays.retain(|entry| entry.id != selected_delay);
                    warnings.push(format!("Delay {selected_delay} skipped: {error}"));
                }
            }
        }

        let selected_scanner = self.selected.scanner.clone();
        if !selected_scanner.is_empty()
            && let Some(scanner) = raw.pointer("/instruments/scanner")
        {
            let x = parse_scanner(scanner.get("x"), "X");
            let y = parse_scanner(scanner.get("y"), "Y");
            let zero = raw.pointer("/measurements/move_abs/zero");
            let mut invalid = false;
            let x = match x {
                Ok(config) => config,
                Err(error) => {
                    warnings.push(format!("Scanner {selected_scanner} X skipped: {error}"));
                    invalid = true;
                    None
                }
            };
            let y = match y {
                Ok(config) => config,
                Err(error) => {
                    warnings.push(format!("Scanner {selected_scanner} Y skipped: {error}"));
                    invalid = true;
                    None
                }
            };
            let no_valid_axis = x.is_none() && y.is_none();
            match (number(zero, "x_um"), number(zero, "y_um")) {
                (Ok(x_zero), Ok(y_zero)) => {
                    if let Some(entry) = self.scanners.iter_mut().find(|e| e.id == selected_scanner)
                    {
                        entry.x = x;
                        entry.y = y;
                        entry.zero_um = [x_zero, y_zero];
                        entry.raw_x = scanner.get("x").cloned();
                        entry.raw_y = scanner.get("y").cloned();
                    }
                }
                (x_zero, y_zero) => {
                    self.scanners.retain(|entry| entry.id != selected_scanner);
                    let error = x_zero
                        .err()
                        .or_else(|| y_zero.err())
                        .expect("one zero failed");
                    warnings.push(format!("Scanner {selected_scanner} skipped: {error}"));
                }
            }
            if invalid && no_valid_axis {
                self.scanners.retain(|entry| entry.id != selected_scanner);
            }
        }
    }

    fn remove_invalid_connection_entries(&mut self, warnings: &mut Vec<String>) {
        self.lockins.retain(|entry| {
            let valid = (1..=30).contains(&entry.address);
            if !valid {
                warnings.push(format!(
                    "Lock-in {} skipped: GPIB address must be 1..30",
                    entry.id
                ));
            }
            valid
        });
        self.delays.retain(|entry| {
            match entry
                .config
                .normalized()
                .and_then(|_| finite(entry.zero_ps, "Delay zero"))
            {
                Ok(()) => true,
                Err(error) => {
                    warnings.push(format!("Delay {} skipped: {error}", entry.id));
                    false
                }
            }
        });
        for entry in &mut self.scanners {
            for (axis, config) in [("X", &mut entry.x), ("Y", &mut entry.y)] {
                if let Some(value) = config
                    && let Err(error) = value.normalized()
                {
                    *config = None;
                    warnings.push(format!("Scanner {} {axis} skipped: {error}", entry.id));
                }
            }
        }
        self.scanners.retain(|entry| {
            let valid = entry.zero_um.into_iter().all(f64::is_finite)
                && (entry.x.is_some() || entry.y.is_some());
            if !valid {
                warnings.push(format!(
                    "Scanner {} skipped: no valid axis or zero",
                    entry.id
                ));
            }
            valid
        });
        self.power_meters
            .retain(|entry| match entry.config.validate(true) {
                Ok(()) => true,
                Err(error) => {
                    warnings.push(format!("Power meter {} skipped: {error}", entry.id));
                    false
                }
            });
    }
}
