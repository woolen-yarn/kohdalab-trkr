//! GUI-side RPC dispatch. This shares the normal controller and run lifecycle.
use super::*;
use crate::remote::Server;

pub(super) struct Bridge {
    server: Server,
    runs: VecDeque<Value>,
    active: Option<String>,
    phase: String,
    used_ids: std::collections::HashSet<String>,
}
impl Bridge {
    fn new(server: Server) -> Self {
        Self {
            server,
            runs: VecDeque::new(),
            active: None,
            phase: String::new(),
            used_ids: Default::default(),
        }
    }
    pub(super) fn observe(&mut self, event: &Event) {
        if self.active.is_none() {
            return;
        }
        if let Some(run) = self.runs.back_mut() {
            if let Event::Sample(_) = event {
                run["rows"] = json!(run["rows"].as_u64().unwrap_or(0) + 1);
            }
            if let Event::State(state) = event {
                self.phase = state.phase.clone();
                run["rows"] = json!(state.rows);
            }
        }
        if let Event::Finished(result) = event {
            if let Some(run) = self.runs.back_mut() {
                match result {
                    Ok((rows, output)) => {
                        run["status"] = json!(if self.phase == "Stopped" {
                            "stopped"
                        } else {
                            "completed"
                        });
                        run["rows"] = json!(rows);
                        if let Some(output) = output {
                            run["output"] = json!(output);
                        } else if !run["output"]
                            .as_str()
                            .is_some_and(|p| std::path::Path::new(p).is_file())
                        {
                            run["output"] = Value::Null;
                        }
                    }
                    Err(error) => {
                        run["status"] = json!("error");
                        run["error"] = json!(error);
                    }
                }
            }
            self.active = None;
        }
    }
    fn dispatch(&mut self, app: &mut App, request: crate::remote::Request) {
        let result = if Instant::now() >= request.deadline {
            Err("GUI request expired before execution".into())
        } else {
            self.handle(app, &request.body)
        };
        let response = match result {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        let _ = request.reply.send(response);
    }
    fn run(&self, id: &str) -> Result<Value, String> {
        self.runs
            .iter()
            .find(|r| r["id"] == id)
            .cloned()
            .ok_or_else(|| "Unknown or expired run ID".into())
    }
    fn handle(&mut self, app: &mut App, body: &Value) -> Result<Value, String> {
        match body["action"].as_str().ok_or("Missing action")? {
            "status" => Ok(
                json!({"snapshot":app.snapshot,"active_run":self.active.as_ref().and_then(|id|self.run(id).ok())}),
            ),
            "plan" => {
                let workflow = selected_workflow(body, app.workflow)?;
                Ok(
                    json!({"workflow":workflow.key(),"plan":app.plans[App::ix(workflow)],"source":app.source()}),
                )
            }
            "start" => {
                let id = body["request_id"]
                    .as_str()
                    .ok_or("start requires a unique request_id")?;
                uuid::Uuid::parse_str(id).map_err(|_| "request_id must be a UUID")?;
                // A repeated request returns the original receipt, never another scan.
                if let Ok(existing) = self.run(id) {
                    return Ok(existing);
                }
                if self.used_ids.contains(id) {
                    return Err("Run receipt expired; this request_id cannot be reused".into());
                }
                if self.used_ids.len() >= 100_000 {
                    return Err("GUI API session run limit reached; restart the GUI".into());
                }
                if app.snapshot.running || app.measurement_pending || self.active.is_some() {
                    return Err("GUI is busy".into());
                }
                let workflow = selected_workflow(body, app.workflow)?;
                let plan: Plan = if body.get("plan").is_some_and(|p| !p.is_null()) {
                    serde_json::from_value(body["plan"].clone())
                        .map_err(|e| format!("Invalid plan: {e}"))?
                } else {
                    app.plans[App::ix(workflow)].clone()
                };
                if plan.workflow != workflow {
                    return Err("Plan workflow does not match selected workflow".into());
                }
                plan.targets()?;
                if !app.source_demo
                    && (!app.snapshot.devices[0]
                        || engine::active_axes(&plan)
                            .iter()
                            .enumerate()
                            .any(|(i, needed)| *needed && !app.snapshot.devices[i + 1]))
                {
                    return Err("Connect the required devices in the GUI before starting".into());
                }
                // Use exactly the GUI's selected source and device configuration.
                // Only measurement plans/output paths are exposed to external clients.
                let output = match body.get("output") {
                    None | Some(Value::Null) => {
                        let previous = app.workflow;
                        app.workflow = workflow;
                        let output = app.measurement_output();
                        app.workflow = previous;
                        output
                    }
                    Some(Value::String(path)) if !path.trim().is_empty() => {
                        Some(PathBuf::from(path))
                    }
                    _ => return Err("output must be a nonempty CSV path".into()),
                };
                app.begin_measurement(plan, output.clone())?;
                let receipt =
                    json!({"id":id,"status":"running","rows":0,"output":output,"error":null});
                if self.runs.len() == 32 {
                    self.runs.pop_front();
                }
                self.runs.push_back(receipt.clone());
                self.used_ids.insert(id.into());
                self.active = Some(id.into());
                self.phase.clear();
                Ok(receipt)
            }
            "result" => self.run(body["run_id"].as_str().ok_or("Missing run_id")?),
            "stop" => {
                let id = body["run_id"].as_str().ok_or("Missing run_id")?;
                let result = self.run(id)?;
                if self.active.as_deref() != Some(id) {
                    return Err("This run is no longer active".into());
                }
                app.controller.stop();
                Ok(result)
            }
            _ => Err("Unknown GUI API action".into()),
        }
    }
}
fn selected_workflow(body: &Value, current: Workflow) -> Result<Workflow, String> {
    match body.get("workflow") {
        None | Some(Value::Null) => Ok(current),
        Some(Value::String(key)) => Workflow::ALL
            .into_iter()
            .find(|w| w.key() == key)
            .ok_or_else(|| "Unknown workflow".into()),
        _ => Err("workflow must be a workflow key".into()),
    }
}
impl App {
    pub fn enable_remote(&mut self, server: Server) {
        self.remote = Some(Bridge::new(server));
    }
    pub(super) fn poll_remote(&mut self) {
        let Some(mut bridge) = self.remote.take() else {
            return;
        };
        for _ in 0..8 {
            let Ok(request) = bridge.server.requests.try_recv() else {
                break;
            };
            bridge.dispatch(self, request);
        }
        self.remote = Some(bridge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bridge() -> (tempfile::TempDir, Bridge) {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::in_directory(dir.path().into()).unwrap();
        (dir, Bridge::new(server))
    }
    fn demo_app() -> App {
        let mut app = App::with_context(&egui::Context::default(), false);
        app.source_demo = true;
        app.plans[App::ix(Workflow::SignalMonitor)].n_points = 3;
        app.plans[App::ix(Workflow::SignalMonitor)].interval_s = 0.;
        app
    }
    fn finish(app: &mut App, bridge: &mut Bridge) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while bridge.active.is_some() {
            let event = app
                .controller
                .events
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            bridge.observe(&event);
            if matches!(event, Event::Finished(_)) {
                app.measurement_pending = false;
            }
            if let Event::State(snapshot) = event {
                app.apply_snapshot(*snapshot);
            }
            assert!(Instant::now() < deadline);
        }
    }
    #[test]
    fn cross_workflow_plan_updates_shared_coordinates_and_saved_profile() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let dir = tempfile::tempdir().unwrap();
        let mut plan = app.plans[App::ix(Workflow::Srkr)].clone();
        plan.zero = [12., 34., 56.];
        plan.theta_deg = 42.;
        plan.interval_s = 0.;
        plan.fast_range = Range {
            min: 0.,
            max: 1.,
            step: 1.,
        };
        bridge.handle(&mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"workflow":"srkr","plan":plan,"output":dir.path().join("srkr.csv")})).unwrap();
        finish(&mut app, &mut bridge);
        for workflow in Workflow::ALL {
            let view = bridge
                .handle(
                    &mut app,
                    &json!({"action":"plan","workflow":workflow.key()}),
                )
                .unwrap();
            assert_eq!(view["plan"]["zero"], json!(plan.zero));
            assert_eq!(view["plan"]["theta_deg"], json!(42.));
        }
        app.sync_all_plans().unwrap();
        let profile = dir.path().join("profile.json");
        app.settings.save(&profile).unwrap();
        let saved = Settings::load(&profile).unwrap();
        assert_eq!(saved.plan(Workflow::Strkr).unwrap().zero, plan.zero);
        assert_eq!(saved.plan(Workflow::Strkr).unwrap().theta_deg, 42.);
        // A second workflow without an explicit plan uses the same coordinates.
        bridge.handle(&mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"workflow":"signal_monitor","output":dir.path().join("monitor.csv")})).unwrap();
        assert_eq!(app.plan().zero, plan.zero);
        finish(&mut app, &mut bridge);
    }
    #[test]
    fn expired_start_never_executes_and_manual_job_blocks_external_start() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let (reply, receive) = std::sync::mpsc::sync_channel(1);
        bridge.dispatch(
            &mut app,
            crate::remote::Request {
                body: json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string()}),
                deadline: Instant::now() - Duration::from_secs(1),
                reply,
            },
        );
        assert_eq!(receive.recv().unwrap()["ok"], false);
        assert!(!app.snapshot.running);
        assert!(bridge.runs.is_empty());
        // Final State can arrive before the preceding GUI job's Finished.
        // Do not associate that old Finished event with a newly started API job.
        app.snapshot.running = false;
        app.measurement_pending = true;
        assert!(
            bridge
                .handle(
                    &mut app,
                    &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string()})
                )
                .unwrap_err()
                .contains("busy")
        );
    }
    #[test]
    fn external_start_uses_normal_gui_event_and_plot_updates() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let dir = tempfile::tempdir().unwrap();
        let receipt = bridge.handle(&mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":dir.path().join("plot.csv")})).unwrap();
        app.remote = Some(bridge);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.remote.as_ref().unwrap().active.is_some() {
            app.receive();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.trace.len(), 3);
        assert_eq!(app.scan_progress[0], (3, 3));
        assert!(!app.snapshot.running);
        let mut bridge = app.remote.take().unwrap();
        assert_eq!(
            bridge.run(receipt["id"].as_str().unwrap()).unwrap()["rows"],
            3
        );
        bridge.runs.clear();
        assert!(
            bridge
                .handle(
                    &mut app,
                    &json!({"action":"start","request_id":receipt["id"]})
                )
                .unwrap_err()
                .contains("cannot be reused")
        );
    }
    #[test]
    fn starts_one_gui_job_and_records_completion_without_restarting_retries() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("field_0.5T.csv");
        let request =
            json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":output});
        let receipt = bridge.handle(&mut app, &request).unwrap();
        assert!(app.snapshot.running);
        assert_eq!(bridge.handle(&mut app, &request).unwrap(), receipt);
        let other = json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string()});
        assert!(
            bridge
                .handle(&mut app, &other)
                .unwrap_err()
                .contains("busy")
        );
        finish(&mut app, &mut bridge);
        let result = bridge.run(receipt["id"].as_str().unwrap()).unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["rows"], 3);
        assert!(output.exists());
        assert!(output.with_extension("run.json").exists());
        assert_eq!(bridge.handle(&mut app, &request).unwrap(), result);
        assert!(!app.snapshot.running);
    }
    #[test]
    fn stop_and_failed_output_are_reported_without_affecting_another_run() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        app.plans[0].n_points = 100;
        app.plans[0].interval_s = 1.;
        let dir = tempfile::tempdir().unwrap();
        let receipt = bridge.handle(&mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":dir.path().join("stop.csv")})).unwrap();
        assert!(
            bridge
                .handle(&mut app, &json!({"action":"stop","run_id":"wrong"}))
                .is_err()
        );
        // The GUI Stop button uses the same cancellation token.
        app.controller.stop();
        finish(&mut app, &mut bridge);
        assert_eq!(
            bridge.run(receipt["id"].as_str().unwrap()).unwrap()["status"],
            "stopped"
        );
        assert!(
            bridge
                .handle(&mut app, &json!({"action":"stop","run_id":receipt["id"]}))
                .is_err()
        );
        let output = dir.path().join("existing.csv");
        std::fs::write(&output, "preserve").unwrap();
        let failed = bridge.handle(&mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":output})).unwrap();
        finish(&mut app, &mut bridge);
        assert_eq!(
            bridge.run(failed["id"].as_str().unwrap()).unwrap()["status"],
            "error"
        );
        assert_eq!(std::fs::read_to_string(output).unwrap(), "preserve");
    }
    #[test]
    fn invalid_plan_and_unconnected_hardware_do_not_start_or_change_gui() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let mut plan = app.plans[0].clone();
        plan.n_points = 0;
        let request =
            json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"plan":plan});
        assert!(bridge.handle(&mut app, &request).is_err());
        assert!(!app.snapshot.running);
        assert_eq!(app.plans[0].n_points, 3);
        app.source_demo = false;
        let request = json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string()});
        assert!(
            bridge
                .handle(&mut app, &request)
                .unwrap_err()
                .contains("Connect")
        );
        assert!(!app.snapshot.running);
    }
}
