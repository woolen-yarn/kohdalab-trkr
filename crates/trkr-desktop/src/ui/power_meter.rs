//! UI-only controls for the optional optical power meter.
//!
//! The worker owns USB I/O.  This module only stages inventory edits and
//! renders status received through `Event::Power`/`Snapshot`.
use super::*;

const POWER_HISTORY: usize = 180;

impl App {
    fn send_power_meter_operation(&mut self, id: &str, command: Command) -> Result<(), String> {
        if self.source_demo || self.demo_viewer {
            return Err("Select GPIB to connect hardware".into());
        }
        let receipt = format!("power-meter-{}", uuid::Uuid::new_v4());
        self.controller.send_remote(receipt.clone(), command)?;
        self.power_meter_receipts.insert(receipt, id.into());
        Ok(())
    }

    pub(super) fn finish_power_meter_operation(
        &mut self,
        receipt: &str,
        result: &Result<(usize, Option<PathBuf>), String>,
    ) {
        let Some(id) = self.power_meter_receipts.remove(receipt) else {
            return;
        };
        if let Some(pending) = self.power_meter_pending.remove(&id)
            && result.is_ok()
            && let Some(entry) = self
                .inventory
                .power_meters
                .iter_mut()
                .find(|entry| entry.id == id)
        {
            entry.config = pending;
            self.power_meter_drafts.remove(&id);
            if let Err(error) = self.sync_inventory() {
                self.note(error);
            } else {
                self.note("Power-meter settings applied");
            }
        }
        if result.is_err() {
            self.power_meter_disconnect_requested.remove(&id);
        }
    }

    pub(super) fn enable_power_meter_preview(&mut self) {
        let id = "power-meter-preview".to_owned();
        let config = crate::power_meter::Config {
            resource: "USB0::0x1313::0x8078::PREVIEW::INSTR".into(),
            wavelength_nm: 800.,
            correction_factor: 0.75,
        };
        self.inventory
            .power_meters
            .push(crate::inventory::PowerMeterEntry {
                id: id.clone(),
                name: "Power Meter 1".into(),
                label: "Probe".into(),
                config: config.clone(),
            });
        self.inventory.selected.power_meter = id.clone();
        self.inventory
            .project_selected(&mut self.settings.raw)
            .expect("preview inventory");
        for workflow in Workflow::ALL {
            let mut ids = self
                .settings
                .workflow_device_ids(workflow)
                .expect("preview bindings");
            ids.power_meter = id.clone();
            self.settings
                .set_workflow_devices(workflow, ids)
                .expect("preview binding");
        }
        self.power_meter_expanded.insert(id.clone());
        // Only --gui-layout-smoke reaches this fixture; no USB session is opened.
        let start = time::OffsetDateTime::now_utc() - time::Duration::seconds(10);
        for index in 0..40 {
            let time = (start + time::Duration::milliseconds(index * 250))
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap();
            let meter_w = 0.002 + 0.0001 * (index as f64 * 0.2).sin();
            self.observe_power_meter(crate::power_meter::Status {
                id: id.clone(),
                session_id: "preview".into(),
                revision: index as u64 + 1,
                connected: true,
                config: config.clone(),
                identity: json!({"simulation":true,"console_idn":"THORLABS,PM100USB,PREVIEW,0"}),
                error: None,
                reading: Some(crate::power_meter::Reading {
                    native_value: meter_w,
                    native_unit: "W".into(),
                    meter_w,
                    corrected_w: meter_w * 0.75,
                    wavelength_nm: 800.,
                    correction_factor: 0.75,
                    host_started_at_utc: time.clone(),
                    host_completed_at_utc: time,
                    duration_s: 0.01,
                    simulated: true,
                    device_settings: Value::Null,
                }),
            });
        }
    }

    pub(super) fn refresh_power_meter_resources(&mut self, resource: String) {
        #[cfg(feature = "gui-interaction-tests")]
        if self.gui_interaction.sandbox.is_some() {
            self.note("SIMULATED — USB discovery disabled");
            return;
        }
        if self.power_meter_refresh.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(crate::power_meter::resources::discover(&resource));
        });
        self.power_meter_refresh = Some(receiver);
        self.note("Refreshing power meters…");
    }

    pub(super) fn poll_power_meter_resources(&mut self) {
        let Some(receiver) = &self.power_meter_refresh else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.power_meter_refresh = None;
        match result {
            Ok(discovery) => {
                self.power_meter_resources = discovery.resources;
                self.note(format!(
                    "Power meters refreshed: {} found",
                    self.power_meter_resources.len()
                ));
                for warning in discovery.warnings {
                    self.note(warning);
                }
            }
            Err(error) => self.note(format!("Power-meter refresh: {error}")),
        }
    }

    pub(super) fn observe_power_meter(&mut self, status: crate::power_meter::Status) {
        let id = status.id.clone();
        if self.power_meter_retired.contains(&status.session_id) {
            return;
        }
        if let Some(current) = self.power_meter_status(&id) {
            if current.session_id == status.session_id && current.revision > status.revision {
                return;
            }
            if current.session_id != status.session_id {
                self.power_meter_retired.insert(current.session_id.clone());
            }
        }
        if status.connected {
            self.power_meter_disconnected.remove(&id);
        }
        if let Some(reading) = &status.reading {
            let now = Instant::now();
            let history = self.power_meter_history.entry(id.clone()).or_default();
            if history
                .back()
                .is_none_or(|(completed, _)| completed != &reading.host_completed_at_utc)
            {
                history.push_back((reading.host_completed_at_utc.clone(), reading.corrected_w));
                if history.len() > POWER_HISTORY {
                    history.pop_front();
                }
                self.power_meter_seen.insert(id.clone(), now);
            }
        }
        if let Some(existing) = self
            .snapshot
            .power_meters
            .iter_mut()
            .find(|item| item.id == id)
        {
            *existing = status;
        } else {
            self.snapshot.power_meters.push(status);
        }
    }

    pub(super) fn power_meter_status(&self, id: &str) -> Option<&crate::power_meter::Status> {
        self.snapshot
            .power_meters
            .iter()
            .find(|status| status.id == id)
    }

    pub(super) fn power_meter_height(&self, id: &str) -> f32 {
        if self.inventory.selected.power_meter == id {
            if self.power_meter_expanded.contains(id) {
                370.
            } else {
                234.
            }
        } else {
            78.
        }
    }

    fn apply_power_meter_config(&mut self, id: &str) {
        let Some(draft) = self.power_meter_drafts.get(id).cloned() else {
            return;
        };
        let connected = self
            .power_meter_status(id)
            .is_some_and(|status| status.connected);
        if let Err(error) = draft.validate(!connected) {
            self.note(error);
            return;
        }
        if !connected {
            if let Some(entry) = self
                .inventory
                .power_meters
                .iter_mut()
                .find(|entry| entry.id == id)
            {
                entry.config = draft;
                self.power_meter_drafts.remove(id);
                if let Err(error) = self.sync_inventory() {
                    self.note(error);
                } else {
                    self.note("Power-meter settings saved");
                }
            }
            return;
        }
        match self.send_power_meter_operation(
            id,
            Command::ConfigurePower {
                id: id.to_owned(),
                config: draft.clone(),
            },
        ) {
            Ok(()) => {
                self.power_meter_pending.insert(id.to_owned(), draft);
                self.note("Applying power-meter settings…");
            }
            Err(error) => self.note(error),
        }
    }

    pub(super) fn power_meter_card(&mut self, ui: &mut egui::Ui, rect: egui::Rect, id: &str) {
        use precision::rect as local_rect;
        let Some(entry) = self
            .inventory
            .power_meters
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return;
        };
        let status = self.power_meter_status(id).cloned();
        let connected = status.as_ref().is_some_and(|status| status.connected);
        let editable = self.inventory_editable() && !self.power_meter_pending.contains_key(id);
        let origin = rect.min;
        let mut connect = false;
        let mut disconnect = false;
        let mut apply = false;
        let mut refresh = false;
        let mut draft = self
            .power_meter_drafts
            .get(id)
            .cloned()
            .unwrap_or_else(|| entry.config.clone());
        let draft_error = draft.validate(true).err();
        let status_text =
            if let Some(error) = status.as_ref().and_then(|status| status.error.as_deref()) {
                ("ERROR", Color32::LIGHT_RED, error.to_owned())
            } else if !connected {
                ("HELD", Color32::GRAY, "Disconnected".to_owned())
            } else if status
                .as_ref()
                .and_then(|status| status.reading.as_ref())
                .is_none()
            {
                (
                    "HELD",
                    Color32::from_rgb(213, 169, 77),
                    "Connected · awaiting reading".to_owned(),
                )
            } else if self
                .power_meter_seen
                .get(id)
                .is_some_and(|seen| seen.elapsed() < Duration::from_secs(3))
            {
                (
                    if status
                        .as_ref()
                        .and_then(|s| s.reading.as_ref())
                        .is_some_and(|r| r.simulated)
                    {
                        "SIM"
                    } else {
                        "LIVE"
                    },
                    Color32::from_rgb(93, 196, 146),
                    "Corrected sample-plane power".to_owned(),
                )
            } else {
                (
                    "STALE",
                    Color32::from_rgb(213, 169, 77),
                    "Last reading has not refreshed".to_owned(),
                )
            };
        let mut form = precision::Form::new(ui, &mut self.geometry, origin);
        let mut expanded = self.power_meter_expanded.contains(id);
        form.enabled = !self.demo_viewer;
        let w = rect.width();
        form.toggle(
            "power_device",
            local_rect(11., 27., 71., 24.),
            &mut expanded,
            "Device",
        );
        form.value(
            "power_device_name",
            local_rect(102., 27., w - 113., 24.),
            status
                .as_ref()
                .filter(|s| s.connected)
                .and_then(|s| {
                    s.identity["console_idn"]
                        .as_str()
                        .and_then(|v| v.split(',').nth(1))
                        .or_else(|| s.identity["console"]["model"].as_str())
                })
                .unwrap_or("—"),
            TEXT,
        );
        form.ui.painter().circle_filled(
            rect.min + Vec2::new(93., 39.),
            3.,
            if connected {
                Color32::from_rgb(100, 196, 146)
            } else {
                Color32::GRAY
            },
        );
        if expanded {
            form.enabled = editable;
            form.label(
                "power_resource_label",
                local_rect(20., 66., 72., 26.),
                "USB resource",
            );
            form.editable_combo(
                "power_resource",
                local_rect(96., 66., w - 197., 26.),
                &mut draft.resource,
                &self.power_meter_resources,
            );
            refresh = form.button(
                "power_refresh",
                local_rect(w - 95., 66., 84., 26.),
                "Refresh",
                editable,
            );
            form.label(
                "power_wavelength_label",
                local_rect(20., 98., 72., 26.),
                "Wavelength",
            );
            form.number(
                "power_wavelength",
                local_rect(96., 98., w - 141., 26.),
                &mut draft.wavelength_nm,
                1.,
                3,
                editable,
            );
            form.value(
                "power_wavelength_unit",
                local_rect(w - 40., 98., 30., 26.),
                "nm",
                Color32::GRAY,
            );
            form.label(
                "power_factor_label",
                local_rect(20., 130., 72., 26.),
                "Correction",
            );
            form.number(
                "power_factor",
                local_rect(96., 130., w - 107., 26.),
                &mut draft.correction_factor,
                0.01,
                5,
                editable,
            );
            let buttons_y = 162.;
            let button_w = (w - 34.) / 3.;
            connect = form.button(
                "power_connect",
                local_rect(11., buttons_y, button_w, 26.),
                "Connect",
                editable && !connected && draft.validate(false).is_ok(),
            );
            disconnect = form.button(
                "power_disconnect",
                local_rect(17. + button_w, buttons_y, button_w, 26.),
                "Disconnect",
                editable && connected,
            );
            apply = form.button(
                "power_apply",
                local_rect(23. + button_w * 2., buttons_y, button_w, 26.),
                if self.power_meter_pending.contains_key(id) {
                    "Applying…"
                } else {
                    "Apply"
                },
                editable && draft_error.is_none(),
            );
        }
        let live_y = if expanded { 202. } else { 54. };
        let area = form.at("power_live", local_rect(11., live_y, w - 22., 140.));
        let current = status
            .as_ref()
            .and_then(|s| s.reading.as_ref())
            .map(|r| r.corrected_w)
            .filter(|v| v.is_finite());
        let simulated = status
            .as_ref()
            .and_then(|s| s.reading.as_ref())
            .is_some_and(|r| r.simulated);
        form.value(
            "power_status_detail",
            local_rect(11., live_y + 144., w - 22., 16.),
            draft_error.unwrap_or(status_text.2.clone()),
            status_text.1,
        );
        let config = draft.clone();
        self.paint_power_plot(ui, area, id, current, connected, simulated, status_text.0);
        if expanded {
            self.power_meter_expanded.insert(id.to_owned());
        } else {
            self.power_meter_expanded.remove(id);
        }
        self.power_meter_drafts
            .insert(id.to_owned(), config.clone());
        if refresh {
            self.refresh_power_meter_resources(config.resource.clone());
        }
        if apply {
            self.apply_power_meter_config(id);
        }
        if connect {
            self.power_meter_pending
                .insert(id.to_owned(), config.clone());
            match self.send_power_meter_operation(
                id,
                Command::ConnectPower {
                    id: id.to_owned(),
                    config: config.clone(),
                },
            ) {
                Ok(()) => {
                    self.note("Connecting power meter…");
                }
                Err(error) => {
                    self.power_meter_pending.remove(id);
                    self.note(error);
                }
            }
        }
        if disconnect {
            match self
                .send_power_meter_operation(id, Command::DisconnectPower { id: id.to_owned() })
            {
                Ok(()) => {
                    self.power_meter_disconnect_requested.insert(id.to_owned());
                    self.note("Disconnecting power meter…");
                }
                Err(error) => self.note(error),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_power_plot(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        id: &str,
        current: Option<f64>,
        active: bool,
        simulated: bool,
        state: &str,
    ) {
        let history = self.power_meter_history.get(id);
        let now = time::OffsetDateTime::now_utc().unix_timestamp_nanos() as f64 / 1e9;
        let last = history
            .and_then(|h| h.back())
            .map(|(t, _)| timestamp_seconds(t));
        let newest = if active { now } else { last.unwrap_or(now) };
        let points: Vec<[f64; 2]> = history
            .into_iter()
            .flatten()
            .map(|(completed, watts)| [timestamp_seconds(completed) - newest, *watts])
            .filter(|point| point[0] >= -30. && point[1].is_finite())
            .collect();
        let maximum = points
            .iter()
            .fold(0_f64, |extent, p| extent.max(p[1].abs()));
        let (scale, prefix) = power_scale(maximum);
        let window_seconds = points.first().map_or(2., |p| -p[0]);
        crate::live_view::draw_channel(
            ui,
            rect,
            crate::live_view::LiveChannel {
                id: egui::Id::new(("power-meter-live", id)),
                name: "P".into(),
                origin: if simulated { "SIM" } else { "MEASURED" }.into(),
                state: if state == "SIM" { "LIVE" } else { state }.into(),
                active,
                unit: "W".into(),
                prefix,
                display: current.map_or_else(|| "— W".into(), format_power),
                current,
                points,
                age_seconds: self
                    .power_meter_seen
                    .get(id)
                    .map(|seen| seen.elapsed().as_secs_f64()),
                window_seconds,
                scale,
                color: crate::live_view::TEAL,
            },
        );
    }
}

/// RFC3339 wall-clock seconds for display spacing only. The host-provided
/// host completion timestamp is retained verbatim for provenance and de-duplication.
fn timestamp_seconds(value: &str) -> f64 {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(|time| time.unix_timestamp_nanos() as f64 / 1e9)
        .unwrap_or(0.)
}

fn format_power(watts: f64) -> String {
    if !watts.is_finite() {
        return "— W".into();
    }
    let (scale, prefix) = power_scale(watts);
    format!(
        "{} {prefix}W",
        App::significant(watts / scale, crate::power_meter::SIGNIFICANT_DIGITS)
    )
}

fn power_scale(watts: f64) -> (f64, &'static str) {
    if watts.abs() >= 1. {
        (1., "")
    } else if watts.abs() >= 1e-3 {
        (1e-3, "m")
    } else if watts.abs() >= 1e-6 {
        (1e-6, "µ")
    } else if watts.abs() >= 1e-9 {
        (1e-9, "n")
    } else {
        (1e-12, "p")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(id: &str) -> crate::power_meter::Status {
        crate::power_meter::Status {
            id: id.into(),
            connected: true,
            config: crate::power_meter::Config::default(),
            identity: Value::Null,
            reading: None,
            error: None,
            session_id: "test".into(),
            revision: 1,
        }
    }

    #[test]
    fn configuration_is_committed_by_its_receipt_not_a_background_error() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let id = "power-1";
        app.inventory
            .power_meters
            .push(crate::inventory::PowerMeterEntry {
                id: id.into(),
                name: "Power Meter 1".into(),
                label: String::new(),
                config: Default::default(),
            });
        let pending = crate::power_meter::Config {
            correction_factor: 0.75,
            ..Default::default()
        };
        app.power_meter_pending.insert(id.into(), pending.clone());
        app.power_meter_receipts.insert("apply".into(), id.into());
        let mut old = status(id);
        old.error = Some("an earlier read failed".into());
        app.observe_power_meter(old);
        assert!(app.power_meter_pending.contains_key(id));
        assert_eq!(app.inventory.power_meters[0].config.correction_factor, 1.);
        app.finish_power_meter_operation("apply", &Ok((0, None)));
        assert_eq!(app.inventory.power_meters[0].config, pending);
        assert_eq!(
            app.settings.raw["device_inventory"]["power_meters"][0]["config"]["correction_factor"],
            0.75
        );
        app.power_meter_pending
            .insert(id.into(), Default::default());
        app.power_meter_receipts.insert("bad".into(), id.into());
        app.finish_power_meter_operation("bad", &Err("not applied".into()));
        assert!(!app.power_meter_pending.contains_key(id));
        assert_eq!(app.inventory.power_meters[0].config, pending);
    }

    #[test]
    fn a_new_snapshot_retires_the_previous_power_worker() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let old = status("power-1");
        app.observe_power_meter(old.clone());
        let mut new = old.clone();
        new.session_id = "new-session".into();
        app.apply_snapshot(Snapshot {
            power_meters: vec![new],
            ..Default::default()
        });
        app.observe_power_meter(old.clone());
        app.apply_snapshot(Snapshot {
            power_meters: vec![old],
            ..Default::default()
        });
        assert_eq!(
            app.power_meter_status("power-1").unwrap().session_id,
            "new-session"
        );
    }

    #[test]
    fn power_events_cannot_revive_retired_sessions_or_erase_new_errors() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let mut old = status("power-1");
        app.observe_power_meter(old.clone());
        let mut newer = old.clone();
        newer.revision = 2;
        newer.error = Some("overload".into());
        app.observe_power_meter(newer.clone());
        app.observe_power_meter(old.clone());
        assert_eq!(
            app.power_meter_status("power-1").unwrap().error.as_deref(),
            Some("overload")
        );
        newer.session_id = "reconnected".into();
        newer.revision = 1;
        newer.error = None;
        app.observe_power_meter(newer);
        old.revision = 99;
        app.observe_power_meter(old);
        assert_eq!(
            app.power_meter_status("power-1").unwrap().session_id,
            "reconnected"
        );
        assert!(app.power_meter_status("power-1").unwrap().error.is_none());
    }

    #[test]
    fn host_plot_time_keeps_real_month_boundaries() {
        assert_eq!(
            timestamp_seconds("2026-03-01T00:00:00Z") - timestamp_seconds("2026-02-28T23:59:59Z"),
            1.
        );
    }

    #[test]
    fn disconnected_meter_is_not_revived_by_an_older_snapshot() {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.snapshot.power_meters = vec![status("power-meter-1")];
        app.power_meter_disconnect_requested
            .insert("power-meter-1".into());
        app.apply_snapshot(Snapshot::default());
        assert!(app.snapshot.power_meters.is_empty());
        app.apply_snapshot(Snapshot {
            power_meters: vec![status("power-meter-1")],
            ..Default::default()
        });
        assert!(app.snapshot.power_meters.is_empty());
    }

    #[test]
    fn power_display_uses_si_prefixes_without_zero_substitution() {
        assert_eq!(format_power(2.5e-3), "2.5 mW");
        assert_eq!(format_power(f64::NAN), "— W");
    }

    #[test]
    fn selected_card_reserves_live_channel_and_settings_without_overlap() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let id = "power-meter-1";
        app.inventory.selected.power_meter = id.into();
        assert!(app.power_meter_height(id) >= 54. + 140. + 16. + 8.);
        app.power_meter_expanded.insert(id.into());
        app.device_expanded[0] = true;
        assert!(app.power_meter_height(id) >= 202. + 140. + 16. + 8.);
    }

    #[test]
    fn live_history_keeps_equal_power_at_distinct_meter_timestamps() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let mut first = status("power-meter-1");
        first.reading = Some(crate::power_meter::Reading {
            native_value: 1.,
            native_unit: "W".into(),
            meter_w: 1.,
            corrected_w: 1.,
            wavelength_nm: 800.,
            correction_factor: 1.,
            host_started_at_utc: "2026-01-01T00:00:00Z".into(),
            host_completed_at_utc: "2026-01-01T00:00:01Z".into(),
            duration_s: 0.1,
            simulated: false,
            device_settings: Value::Null,
        });
        let mut second = first.clone();
        second.revision = 2;
        second.reading.as_mut().unwrap().host_completed_at_utc = "2026-01-01T00:00:03Z".into();
        app.observe_power_meter(first);
        app.observe_power_meter(second);
        assert_eq!(app.power_meter_history["power-meter-1"].len(), 2);
    }
}
