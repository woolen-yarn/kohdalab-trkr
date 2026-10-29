//! The GUI owns all hardware. A renewable Python lease serializes edits and
//! operations; tagged worker completions cannot be confused with idle updates.
use super::*;
use crate::remote::Server;
const LEASE: Duration = Duration::from_secs(6);
#[derive(Debug)]
struct ApiError {
    code: &'static str,
    message: String,
}
impl ApiError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self::new("invalid_request", message)
    }
}
impl From<&str> for ApiError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
#[derive(Clone, Copy, Default, serde::Serialize)]
struct StopPolicy {
    stop_on_disconnect: bool,
    stop_on_error: bool,
    stop_on_timeout: bool,
}
impl StopPolicy {
    fn from_body(body: &Value) -> Result<Self, String> {
        Ok(Self {
            stop_on_disconnect: bool_field(body, "stop_on_disconnect", false)?,
            stop_on_error: bool_field(body, "stop_on_error", false)?,
            stop_on_timeout: bool_field(body, "stop_on_timeout", false)?,
        })
    }
}
struct Owner {
    id: String,
    lease: String,
    label: String,
    until: Instant,
    released: bool,
    policy: StopPolicy,
}
pub(super) struct Bridge {
    server: Server,
    runs: VecDeque<Value>,
    active: Option<String>,
    used_ids: std::collections::HashSet<String>,
    owner: Option<Owner>,
    revoked: std::collections::HashSet<String>,
}
impl Bridge {
    fn new(server: Server) -> Self {
        Self {
            server,
            runs: VecDeque::new(),
            active: None,
            used_ids: Default::default(),
            owner: None,
            revoked: Default::default(),
        }
    }
    fn tick(&mut self) {
        if self.active.is_none()
            && self
                .owner
                .as_ref()
                .is_some_and(|o| o.released || Instant::now() >= o.until)
        {
            self.owner = None;
        }
    }
    pub(super) fn locked(&self) -> bool {
        self.owner.is_some()
    }
    fn request_stop(&mut self, app: &App, reason: &str) {
        if self.active.is_some() {
            if let Some(run) = self.runs.back_mut() {
                if run["stop_requested"] == true {
                    return;
                }
                run["stop_requested"] = json!(true);
                run["stop_reason"] = json!(reason);
            }
            app.controller.stop();
        }
    }
    fn poll_policy(&mut self, app: &App) {
        if self.owner.as_ref().is_some_and(|o| {
            !o.released && Instant::now() >= o.until && o.policy.stop_on_disconnect
        }) {
            self.request_stop(app, "disconnect");
        }
        self.tick();
    }
    fn control(&self) -> Value {
        json!({"owned":self.locked(),"client_id":self.owner.as_ref().map(|o|&o.id),"label":self.owner.as_ref().map(|o|&o.label),
            "status": if self.owner.as_ref().is_some_and(|o|o.released || Instant::now()>=o.until) {"detached"} else if self.locked() {"python"} else {"idle"},"active_run":self.active.as_ref().and_then(|id|self.run(id).ok()),"stop_policy":self.owner.as_ref().map(|o|o.policy)})
    }
    pub(super) fn observe(&mut self, event: &Event) {
        let Some(id) = &self.active else {
            return;
        };
        if let Some(run) = self.runs.back_mut() {
            if run["kind"] == "start"
                && let Event::Sample(_) = event
            {
                run["rows"] = json!(run["rows"].as_u64().unwrap_or(0) + 1);
            }
            if let Event::RemoteFinished {
                id: finished,
                phase,
                result,
                error_code,
            } = event
            {
                if finished != id {
                    return;
                }
                match result {
                    Ok((rows, output)) => {
                        run["status"] = json!(if phase == "Stopped" {
                            "stopped"
                        } else {
                            "completed"
                        });
                        run["rows"] = json!(rows);
                        if let Some(output) = output {
                            run["output"] = json!(output);
                        } else if !run["output"]
                            .as_str()
                            .is_some_and(|p| PathBuf::from(p).is_file())
                        {
                            run["output"] = Value::Null;
                        }
                    }
                    Err(error) => {
                        run["status"] = json!("error");
                        run["error"] = json!(error);
                        run["error_code"] = json!(error_code);
                    }
                }
                self.active = None;
            }
        }
        self.tick();
    }
    fn dispatch(&mut self, app: &mut App, request: crate::remote::Request) {
        let result = if Instant::now() >= request.deadline {
            Err(ApiError::new(
                "request_expired",
                "GUI request expired before execution",
            ))
        } else {
            self.handle(app, &request.body)
        };
        let _ = request.reply.send(match result {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error.message,"error_code":error.code}),
        });
    }
    fn run(&self, id: &str) -> Result<Value, ApiError> {
        self.runs
            .iter()
            .find(|r| r["id"] == id)
            .cloned()
            .ok_or_else(|| ApiError::new("unknown_run", "Unknown or expired run ID"))
    }
    fn idle(&self, app: &App) -> Result<(), ApiError> {
        if app.snapshot.running
            || app.measurement_pending
            || (self.owner.is_none() && app.controller.has_pending())
            || self.active.is_some()
        {
            Err(ApiError::new("busy", "GUI is busy"))
        } else {
            Ok(())
        }
    }
    fn require_owner(&self, body: &Value) -> Result<(), ApiError> {
        if self.owner.as_ref().is_some_and(|o| {
            !o.released
                && Instant::now() < o.until
                && body["client_id"].as_str() == Some(o.id.as_str())
                && (!body.get("_control_method").is_some()
                    || body["lease"].as_str() == Some(o.lease.as_str()))
        }) {
            Ok(())
        } else {
            Err(ApiError::new(
                "ownership_lost",
                "Python control is not owned by this client; acquire control first",
            ))
        }
    }
    fn config_view(app: &mut App) -> Result<Value, String> {
        app.sync_all_plans()?;
        Ok(
            json!({"profile":app.settings.raw,"workflow":app.workflow.key(),"source":if app.source_demo {"demo"} else {"hardware"},"config_path":app.config_path}),
        )
    }
    fn handle(&mut self, app: &mut App, body: &Value) -> Result<Value, ApiError> {
        self.poll_policy(app);
        let action = body["action"].as_str().ok_or("Missing action")?;
        let common = body.get("_control_method").is_some();
        match action {
            "control_describe" => {
                return Ok(json!({
                    "protocol":"kohdalab-control","control_protocol":1,
                    "application":"kohdalab-trkr","application_version":env!("CARGO_PKG_VERSION"),
                    "api_version":1,"methods":["control.describe","control.status","control.plan","control.config","control.output","control.acquire","control.heartbeat","control.release","control.result","control.configure","control.set_plan","control.set_output","control.connect","control.disconnect","control.start","control.stop","control.move","control.initialize","control.read","control.lockin_settings","control.set_lockin","control.auto_lockin"],
                    "safety":{"lease_timeout_seconds":6,"heartbeat_seconds":2,"on_disconnect":"continue","on_release":"continue","stop_requires_lease":true,"stop_policy_defaults":{"stop_on_disconnect":false,"stop_on_error":false,"stop_on_timeout":false}},
                    "native":{"actions":["status","plan","config","output","result","acquire","heartbeat","release","start","stop","connect","disconnect","move","initialize","read","lockin_settings","set_lockin","auto_lockin"]}
                }));
            }
            "control_status" => {
                return Ok(
                    json!({"phase":if self.active.is_some(){"running"}else{"ready"},"busy":self.active.is_some()||app.snapshot.running||app.measurement_pending,"ownership":self.control(),"cleanup":"not_required","native":self.control()}),
                );
            }
            "control_result" => {
                let id = body["run_id"].as_str().ok_or("Missing run_id")?;
                let native = self.run(id)?;
                let state = native["status"].as_str().unwrap_or("error").to_owned();
                let kind = native["kind"].as_str().unwrap_or("run").to_owned();
                let cleanup = if state == "completed" {
                    "verified"
                } else {
                    "pending"
                };
                return Ok(kohdalab_control_protocol::receipt(
                    id, &kind, &state, cleanup, None, native,
                ));
            }
            "status" => {
                return Ok(
                    json!({"snapshot":app.snapshot,"active_run":self.active.as_ref().and_then(|id|self.run(id).ok()),"control":self.control()}),
                );
            }
            "plan" => {
                let w = selected_workflow(body, app.workflow)?;
                return Ok(
                    json!({"workflow":w.key(),"plan":app.plans[App::ix(w)],"source":app.source()}),
                );
            }
            "config" => return Ok(Self::config_view(app)?),
            "output" => {
                let w = selected_workflow(body, app.workflow)?;
                let o = &app.outputs[App::ix(w)];
                return Ok(
                    json!({"dir":o.dir,"filename":o.filename,"auto_timestamp_suffix":o.auto_timestamp_suffix}),
                );
            }
            "result" => return self.run(body["run_id"].as_str().ok_or("Missing run_id")?),
            "acquire" => {
                let id = body["client_id"].as_str().ok_or("Missing client_id")?;
                uuid::Uuid::parse_str(id).map_err(|_| "client_id must be a UUID")?;
                let policy = StopPolicy::from_body(body)?;
                if self.revoked.contains(id) {
                    return Err(ApiError::new(
                        "ownership_lost",
                        "Control was returned to the GUI; create a new client to acquire again",
                    ));
                }
                if let Some(owner) = &mut self.owner {
                    if owner.id != id || owner.released {
                        return Err(ApiError::new(
                            "ownership_lost",
                            "Python control belongs to another client or is finishing",
                        ));
                    }
                    if common
                        && body.get("lease").and_then(Value::as_str) != Some(owner.lease.as_str())
                    {
                        return Err(ApiError::new(
                            "invalid_lease",
                            "Lease does not match the current owner",
                        ));
                    }
                    owner.until = Instant::now() + LEASE;
                    owner.policy = policy;
                } else {
                    self.idle(app)?;
                    if app.dialog.is_some() || app.initialize_axis.is_some() {
                        return Err("Close the GUI dialog before acquiring Python control".into());
                    }
                    let label = body
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or("Python");
                    if label.chars().count() > 48 {
                        return Err("Client label is limited to 48 characters".into());
                    }
                    self.owner = Some(Owner {
                        id: id.into(),
                        lease: uuid::Uuid::new_v4().to_string(),
                        label: label.into(),
                        until: Instant::now() + LEASE,
                        released: false,
                        policy,
                    });
                }
                let mut result = self.control();
                if common {
                    let native = result.clone();
                    result["client_id"] = json!(id);
                    result["lease"] = json!(self.owner.as_ref().map(|o| &o.lease));
                    result["heartbeat_seconds"] = json!(2);
                    result["timeout_seconds"] = json!(6);
                    result["native"] = native;
                }
                return Ok(result);
            }
            "heartbeat" => {
                self.require_owner(body)?;
                self.owner.as_mut().unwrap().until = Instant::now() + LEASE;
                return Ok(self.control());
            }
            "release" => {
                self.require_owner(body)?;
                let reason = body
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("disconnect");
                if !matches!(reason, "disconnect" | "python_error") {
                    return Err("release reason must be disconnect or python_error".into());
                }
                let policy = self.owner.as_ref().unwrap().policy;
                if policy.stop_on_disconnect || (reason == "python_error" && policy.stop_on_error) {
                    self.request_stop(app, reason);
                }
                self.owner.as_mut().unwrap().released = true;
                self.tick();
                let native = self.control();
                return if common {
                    Ok(
                        json!({"released":true,"cleanup":if self.active.is_some(){"pending"}else{"verified"},"native":native}),
                    )
                } else {
                    Ok(native)
                };
            }
            _ => {}
        }
        self.require_owner(body)?;
        if action == "stop" {
            let id = body["run_id"].as_str().ok_or("Missing run_id")?;
            let result = self.run(id)?;
            if self.active.as_deref() != Some(id) {
                return Ok(result); // Stopping a terminal receipt is idempotent.
            }
            self.request_stop(app, "requested");
            return if common {
                Ok(json!({"requested":true,"job_id":id,"cleanup":"pending","native":result}))
            } else {
                self.run(id)
            };
        }
        // A duplicate job returns its original receipt even while that job runs.
        let job = matches!(
            action,
            "start"
                | "move"
                | "initialize"
                | "connect"
                | "disconnect"
                | "read"
                | "lockin_settings"
                | "set_lockin"
                | "auto_lockin"
        );
        let id = if job {
            let id = body["request_id"]
                .as_str()
                .ok_or("Operation requires request_id")?;
            uuid::Uuid::parse_str(id).map_err(|_| "request_id must be a UUID")?;
            if let Ok(existing) = self.run(id) {
                return Ok(existing);
            }
            if self.used_ids.contains(id) {
                return Err("Run receipt expired; this request_id cannot be reused".into());
            }
            if self.used_ids.len() >= 100_000 {
                return Err("GUI API session limit reached; restart GUI".into());
            }
            Some(id.to_owned())
        } else {
            None
        };
        self.idle(app)?;
        match action {
            "load_config" | "configure" => {
                let source = match body.get("source") {
                    None | Some(Value::Null) => app.source_demo,
                    Some(Value::String(s)) if s == "demo" => true,
                    Some(Value::String(s)) if s == "hardware" => false,
                    _ => return Err("source must be demo or hardware".into()),
                };
                if source != app.source_demo && app.snapshot.devices.iter().any(|d| *d) {
                    return Err("Disconnect devices before changing Demo/hardware source".into());
                }
                let loaded = if action == "load_config" {
                    let path = body["path"].as_str().ok_or("Missing config path")?;
                    Some((
                        Settings::load(std::path::Path::new(path))?,
                        Some(path.to_owned()),
                    ))
                } else if let Some(profile) = body.get("profile").filter(|p| !p.is_null()) {
                    Some((Settings::from_value(profile.clone())?, None))
                } else {
                    None
                };
                if let Some((settings, path)) = loaded {
                    if app.snapshot.devices.iter().any(|d| *d) {
                        return Err(
                            "Disconnect devices before replacing the hardware profile".into()
                        );
                    }
                    app.settings = settings;
                    if let Some(path) = path {
                        app.config_path = path;
                    }
                    app.hydrate();
                    app.note(if action == "load_config" {
                        format!("Loaded configuration: {}", app.config_path)
                    } else {
                        "Python configuration applied".into()
                    });
                }
                if source && app.snapshot.devices.iter().any(|d| *d) {
                    return Err("Disconnect hardware before selecting Demo".into());
                }
                app.source_demo = source;
                return Ok(Self::config_view(app)?);
            }
            "set_plan" => {
                let plan: Plan = serde_json::from_value(body["plan"].clone())
                    .map_err(|e| format!("Invalid plan: {e}"))?;
                if body.get("workflow").is_some_and(|w| !w.is_null())
                    && selected_workflow(body, app.workflow)? != plan.workflow
                {
                    return Err("Plan workflow mismatch".into());
                }
                app.sync();
                let mut settings = app.settings.clone();
                settings.set_plan(&plan)?;
                app.switch_workflow(plan.workflow);
                let index = App::ix(plan.workflow);
                app.plans[index] = plan;
                sync_shared_coordinates(&mut app.plans, app.workflow);
                app.settings = settings;
                app.select_plot_axes();
                return Ok(Self::config_view(app)?);
            }
            "set_output" => {
                let w = selected_workflow(body, app.workflow)?;
                let mut o = app.outputs[App::ix(w)].clone();
                if let Some(dir) = body.get("dir") {
                    o.dir = dir
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or("dir must be nonempty")?
                        .into();
                }
                if let Some(name) = body.get("filename") {
                    let name = name
                        .as_str()
                        .map(str::trim)
                        .ok_or("filename must be a string")?;
                    let stem = if name.to_ascii_lowercase().ends_with(".csv") {
                        &name[..name.len() - 4]
                    } else {
                        name
                    };
                    if stem.is_empty() || stem.contains(['/', '\\']) {
                        return Err("filename must be a basename; use dir for the directory".into());
                    }
                    o.filename = stem.into();
                }
                if let Some(auto) = body.get("auto_timestamp_suffix") {
                    o.auto_timestamp_suffix = auto
                        .as_bool()
                        .ok_or("auto_timestamp_suffix must be boolean")?;
                }
                app.outputs[App::ix(w)] = o;
                app.sync_output(w);
                return self.handle(app, &json!({"action":"output","workflow":w.key()}));
            }
            _ => {}
        }
        let id = id.ok_or("Unknown GUI API action")?;
        let mut output = None;
        if action == "start" {
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
                return Err("Connect the required devices before starting".into());
            }
            output = match body.get("output") {
                None | Some(Value::Null) => app.measurement_output_for(workflow),
                Some(Value::String(path)) if !path.trim().is_empty() => Some(PathBuf::from(path)),
                _ => return Err("output must be a CSV path".into()),
            };
            app.begin_measurement_tagged(plan, output.clone(), Some(id.clone()))?;
        } else {
            if action == "connect" && app.source_demo {
                return Err("Select hardware source before connecting devices".into());
            }
            let command = match action {
                "connect" => Command::Connect {
                    config: app.config()?,
                    devices: devices(body, app.configured_devices())?,
                },
                "disconnect" => Command::Disconnect {
                    devices: devices(body, vec![Device::Lockin, Device::T, Device::X, Device::Y])?,
                },
                "read" => Command::Read,
                "lockin_settings" => Command::LockinSettings,
                "set_lockin" => Command::SetLockin {
                    setting: body["setting"].as_str().ok_or("Missing setting")?.into(),
                    value: body["value"].clone(),
                },
                "auto_lockin" => Command::AutoLockin {
                    action: body["auto_action"]
                        .as_str()
                        .ok_or("Missing auto_action")?
                        .into(),
                },
                "move" => {
                    if app.source_demo {
                        return Err("Hardware move is unavailable in Demo mode".into());
                    }
                    let targets: [Option<f64>; 3] = serde_json::from_value(body["targets"].clone())
                        .map_err(|_| "targets must be [t, x, y], with null for unchanged axes")?;
                    if targets.iter().all(Option::is_none) {
                        return Err("Select at least one move axis".into());
                    }
                    let coordinate: Coordinate = serde_json::from_value(
                        body.get("coordinate")
                            .cloned()
                            .unwrap_or(json!("measurement")),
                    )
                    .map_err(|e| e.to_string())?;
                    Command::Move {
                        config: app.config()?,
                        targets,
                        coordinate,
                        zero: app.plan().zero,
                        approach: engine::spatial_approach(),
                    }
                }
                "initialize" => {
                    if app.source_demo {
                        return Err("Hardware initialization is unavailable in Demo mode".into());
                    }
                    let axis = body["axis"]
                        .as_u64()
                        .filter(|a| *a < 3)
                        .ok_or("axis must be 0, 1 or 2")? as usize;
                    Command::Initialize {
                        config: app.config()?,
                        axis,
                        home: bool_field(body, "home", false)?,
                        move_to_origin: bool_field(body, "move_to_origin", true)?,
                    }
                }
                _ => return Err("Unknown GUI API action".into()),
            };
            let motion = matches!(command, Command::Move { .. } | Command::Initialize { .. });
            app.controller.send_remote(id.clone(), command)?;
            if motion {
                app.snapshot.running = true;
            }
        }
        let receipt = json!({"id":id,"status":"running","rows":0,"output":output,"error":null,"error_code":null,"stop_requested":false,"stop_reason":null,"kind":action});
        if self.runs.len() == 32 {
            self.runs.pop_front();
        }
        self.runs.push_back(receipt.clone());
        self.used_ids.insert(id.clone());
        self.active = Some(id);
        if common {
            Ok(kohdalab_control_protocol::receipt(
                self.active.as_deref().unwrap_or(""),
                action,
                "running",
                "pending",
                None,
                receipt,
            ))
        } else {
            Ok(receipt)
        }
    }
    pub(super) fn take_control(&mut self, app: &App) {
        if let Some(owner) = &mut self.owner {
            self.revoked.insert(owner.id.clone());
            owner.released = true;
        }
        if self.active.is_some() {
            app.controller.stop();
        }
        self.tick();
    }
}
fn bool_field(body: &Value, key: &str, default: bool) -> Result<bool, String> {
    body.get(key).map_or(Ok(default), |v| {
        v.as_bool().ok_or_else(|| format!("{key} must be boolean"))
    })
}
fn devices(body: &Value, default: Vec<Device>) -> Result<Vec<Device>, String> {
    match body.get("devices") {
        None | Some(Value::Null) => Ok(default),
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|e| format!("Invalid devices: {e}"))
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
    pub fn enable_remote(&mut self, server: Result<Server, String>) {
        match server {
            Ok(server) => self.remote = Some(Bridge::new(server)),
            Err(error) => self.note(format!("Python API unavailable: {error}")),
        }
    }
    pub(super) fn poll_remote(&mut self) {
        let Some(mut bridge) = self.remote.take() else {
            return;
        };
        bridge.poll_policy(self);
        for _ in 0..8 {
            let Ok(request) = bridge.server.requests.try_recv() else {
                break;
            };
            bridge.dispatch(self, request);
        }
        self.remote = Some(bridge);
    }
    pub(super) fn python_operation_pending(&self) -> bool {
        self.remote.as_ref().is_some_and(|r| r.active.is_some())
    }
    pub(super) fn python_locked(&self) -> bool {
        self.remote.as_ref().is_some_and(Bridge::locked)
    }
    pub(super) fn python_banner(&mut self, ctx: &egui::Context) {
        if !self.python_locked() {
            return;
        }
        let control = self.remote.as_ref().unwrap().control();
        let detached = control["status"] == "detached";
        let mut takeover = false;
        egui::TopBottomPanel::top("python-control-status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(91, 190, 255),
                    if detached && control["active_run"]["stop_requested"] == true {
                        "PYTHON DISCONNECTED · Stopping current operation"
                    } else if detached {
                        "PYTHON DISCONNECTED · Finishing current operation"
                    } else {
                        "PYTHON CONTROL · GUI controls locked"
                    },
                );
                let kind = control["active_run"]["kind"].as_str();
                let activity = match kind {
                    Some("start" | "move" | "initialize") => self.snapshot.phase.as_str(),
                    Some("connect") => "Connecting devices",
                    Some("disconnect") => "Disconnecting devices",
                    Some("set_lockin") => "Updating lock-in",
                    Some("auto_lockin") => "Lock-in auto adjustment",
                    Some("lockin_settings") => "Reading lock-in settings",
                    Some("read") => "Reading devices",
                    _ => "Waiting for Python",
                };
                ui.label(activity);
                if ui.button("Stop").clicked() {
                    self.controller.stop();
                }
                takeover = ui.button("Take control").clicked();
            });
        });
        if takeover && let Some(mut bridge) = self.remote.take() {
            bridge.take_control(self);
            self.remote = Some(bridge);
        }
    }
    pub fn load_startup_profile(&mut self, settings: Settings, path: PathBuf) {
        self.settings = settings;
        self.config_path = path.display().to_string();
        self.hydrate();
        self.log.clear();
        self.note(format!("Loaded configuration: {}", self.config_path));
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
    fn call(bridge: &mut Bridge, app: &mut App, body: &Value) -> Result<Value, String> {
        let mut body = body.clone();
        body["client_id"] = json!("10000000-0000-4000-8000-000000000001");
        bridge
            .handle(
                app,
                &json!({"action":"acquire","client_id":body["client_id"]}),
            )
            .map_err(|e| e.message)?;
        bridge.handle(app, &body).map_err(|e| e.message)
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
    const A: &str = "10000000-0000-4000-8000-000000000001";
    const B: &str = "20000000-0000-4000-8000-000000000002";
    fn long_demo(
        bridge: &mut Bridge,
        app: &mut App,
        policy: Value,
        output: &std::path::Path,
    ) -> String {
        let mut acquire = policy;
        acquire["action"] = json!("acquire");
        acquire["client_id"] = json!(A);
        bridge.handle(app, &acquire).unwrap();
        let mut plan = app.plans[0].clone();
        plan.n_points = 100;
        plan.interval_s = 1.;
        bridge
            .handle(
                app,
                &json!({"action":"start","client_id":A,"workflow":"signal_monitor","plan":plan,
            "request_id":uuid::Uuid::new_v4().to_string(),"output":output}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
    #[test]
    fn disconnect_policy_stops_on_server_expiry_without_a_python_request() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let output = tempfile::tempdir().unwrap();
        let id = long_demo(
            &mut bridge,
            &mut app,
            json!({"stop_on_disconnect":true}),
            &output.path().join("expired.csv"),
        );
        bridge.owner.as_mut().unwrap().until = Instant::now() - Duration::from_secs(1);
        bridge.poll_policy(&app);
        assert_eq!(bridge.run(&id).unwrap()["stop_reason"], "disconnect");
        assert!(bridge.locked()); // No concurrent GUI edits before worker cleanup.
        finish(&mut app, &mut bridge);
        assert_eq!(bridge.run(&id).unwrap()["status"], "stopped");
        assert!(!bridge.locked());
    }
    #[test]
    fn continuation_default_and_python_error_policy_are_independent() {
        for (reason, error_policy, should_stop) in [
            ("disconnect", false, false),
            ("disconnect", true, false),
            ("python_error", false, false),
            ("python_error", true, true),
        ] {
            let (_dir, mut bridge) = bridge();
            let mut app = demo_app();
            let output = tempfile::tempdir().unwrap();
            let id = long_demo(
                &mut bridge,
                &mut app,
                json!({"stop_on_error":error_policy}),
                &output.path().join("policy.csv"),
            );
            bridge
                .handle(
                    &mut app,
                    &json!({"action":"release","client_id":A,"reason":reason}),
                )
                .unwrap();
            assert_eq!(bridge.run(&id).unwrap()["stop_requested"], should_stop);
            if !should_stop {
                bridge.request_stop(&app, "test_cleanup");
            }
            finish(&mut app, &mut bridge);
            assert!(!bridge.locked());
        }
    }
    #[test]
    fn invalid_policy_never_acquires_or_changes_control() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        assert!(
            bridge
                .handle(
                    &mut app,
                    &json!({"action":"acquire","client_id":A,"stop_on_disconnect":"yes"})
                )
                .is_err()
        );
        assert!(!bridge.locked());
    }
    #[test]
    fn common_reacquire_requires_the_opaque_lease() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let acquired = bridge
            .handle(
                &mut app,
                &json!({"action":"acquire","_control_method":"acquire","client_id":A}),
            )
            .unwrap();
        let mut missing = json!({"action":"acquire","_control_method":"acquire","client_id":A});
        assert_eq!(
            bridge.handle(&mut app, &missing).unwrap_err().code,
            "invalid_lease"
        );
        missing["lease"] = acquired["lease"].clone();
        assert!(bridge.handle(&mut app, &missing).is_ok());
    }
    #[test]
    fn common_start_and_result_are_canonical_receipts() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let acquired = bridge
            .handle(
                &mut app,
                &json!({"action":"acquire","_control_method":"acquire","client_id":A}),
            )
            .unwrap();
        let output = tempfile::tempdir().unwrap();
        let request_id = uuid::Uuid::new_v4().to_string();
        let start = bridge
            .handle(
                &mut app,
                &json!({"action":"start","_control_method":"start","client_id":A,
                    "lease":acquired["lease"],"workflow":"signal_monitor",
                    "request_id":request_id,"output":output.path().join("demo.csv")}),
            )
            .unwrap();
        assert_eq!(start["terminal"], false);
        let id = start["job_id"].as_str().unwrap().to_owned();
        finish(&mut app, &mut bridge);
        let result = bridge
            .handle(
                &mut app,
                &json!({"action":"control_result","_control_method":"result","client_id":A,
                    "lease":acquired["lease"],"run_id":id}),
            )
            .unwrap();
        assert_eq!(result["job_id"], id);
        assert_eq!(result["terminal"], true);
        assert_eq!(result["cleanup"], "verified");
    }
    #[test]
    fn common_describe_advertises_existing_configuration_and_device_actions() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let description = bridge
            .handle(
                &mut app,
                &json!({"action":"control_describe","_control_method":"describe"}),
            )
            .unwrap();
        for method in [
            "control.configure",
            "control.set_plan",
            "control.connect",
            "control.start",
        ] {
            assert!(
                description["methods"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == method)
            );
        }
    }
    #[test]
    fn startup_profile_reports_the_explicit_file_instead_of_the_default() {
        let mut app = demo_app();
        app.load_startup_profile(Settings::default(), PathBuf::from("selected-profile.json"));
        assert_eq!(app.config_path, "selected-profile.json");
        assert_eq!(app.log.len(), 1);
        assert_eq!(app.status, "Loaded configuration: selected-profile.json");
    }
    #[test]
    fn unavailable_api_keeps_normal_gui_usable() {
        let mut app = demo_app();
        app.enable_remote(Err("read-only profile directory".into()));
        assert!(!app.python_locked());
        assert!(app.remote.is_none());
        assert!(app.status.contains("Python API unavailable"));
        assert!(!app.snapshot.running);
    }
    #[test]
    fn exclusive_lease_expires_and_gui_takeover_cannot_be_undone_by_heartbeat() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        bridge
            .handle(&mut app, &json!({"action":"acquire","client_id":A}))
            .unwrap();
        assert!(bridge.locked());
        assert!(
            bridge
                .handle(&mut app, &json!({"action":"acquire","client_id":B}))
                .is_err()
        );
        assert!(
            bridge
                .handle(
                    &mut app,
                    &json!({"action":"set_output","client_id":B,"filename":"other"})
                )
                .is_err()
        );
        bridge.owner.as_mut().unwrap().until = Instant::now() - Duration::from_secs(1);
        bridge.tick();
        assert!(!bridge.locked());
        bridge
            .handle(&mut app, &json!({"action":"acquire","client_id":B}))
            .unwrap();
        bridge.take_control(&app);
        assert!(!bridge.locked());
        assert!(
            bridge
                .handle(&mut app, &json!({"action":"heartbeat","client_id":B}))
                .is_err()
        );
        assert!(
            bridge
                .handle(&mut app, &json!({"action":"acquire","client_id":B}))
                .is_err()
        );
    }
    #[test]
    fn release_during_measurement_keeps_gui_locked_until_correlated_completion() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let dir = tempfile::tempdir().unwrap();
        let receipt=call(&mut bridge,&mut app,&json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":dir.path().join("release.csv")})).unwrap();
        bridge
            .handle(&mut app, &json!({"action":"release","client_id":A}))
            .unwrap();
        assert!(bridge.locked());
        assert_eq!(bridge.control()["status"], "detached");
        bridge.observe(&Event::RemoteFinished {
            id: "not-this-operation".into(),
            error_code: None,
            phase: "Complete".into(),
            result: Ok((99, None)),
        });
        assert!(bridge.active.is_some());
        finish(&mut app, &mut bridge);
        assert!(!bridge.locked());
        assert_eq!(
            bridge.run(receipt["id"].as_str().unwrap()).unwrap()["status"],
            "completed"
        );
    }
    #[test]
    fn configuration_output_and_nonmeasurement_operations_share_the_gui() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        let dir = tempfile::tempdir().unwrap();
        let output=call(&mut bridge,&mut app,&json!({"action":"set_output","workflow":"srkr","dir":dir.path(),"filename":"field_1T.csv","auto_timestamp_suffix":false})).unwrap();
        assert_eq!(output["filename"], "field_1T");
        assert_eq!(app.outputs[2].filename, "field_1T");
        assert_eq!(
            app.measurement_output_for(Workflow::Srkr)
                .unwrap()
                .file_name()
                .unwrap(),
            "demo_field_1T.csv"
        );
        app.snapshot.devices = [true, false, false, false];
        assert!(
            call(
                &mut bridge,
                &mut app,
                &json!({"action":"configure","source":"hardware"})
            )
            .is_err()
        );
        assert!(app.source_demo);
        app.snapshot.devices = [false; 4];
        let mut plan = app.plans[2].clone();
        plan.fast_range = Range {
            min: -2.,
            max: 2.,
            step: 1.,
        };
        plan.zero = [2., 3., 4.];
        call(
            &mut bridge,
            &mut app,
            &json!({"action":"set_plan","plan":plan}),
        )
        .unwrap();
        let view = bridge
            .handle(&mut app, &json!({"action":"config"}))
            .unwrap();
        assert_eq!(
            view["profile"]["measurements"]["srkr"]["output"]["filename"],
            "field_1T"
        );
        assert_eq!(app.workflow, Workflow::Srkr);
        assert_eq!(app.plans[4].zero, [2., 3., 4.]);
        let read = call(
            &mut bridge,
            &mut app,
            &json!({"action":"read","request_id":uuid::Uuid::new_v4().to_string()}),
        )
        .unwrap();
        finish(&mut app, &mut bridge);
        let completed = bridge.run(read["id"].as_str().unwrap()).unwrap();
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["kind"], "read");
        let bad=call(&mut bridge,&mut app,&json!({"action":"set_lockin","request_id":uuid::Uuid::new_v4().to_string(),"setting":"Sensitivity","value":1.})).unwrap();
        finish(&mut app, &mut bridge);
        assert_eq!(
            bridge.run(bad["id"].as_str().unwrap()).unwrap()["status"],
            "error"
        );
        let original = app.settings.raw.clone();
        assert!(
            call(
                &mut bridge,
                &mut app,
                &json!({"action":"configure","profile":{"bad":"profile"}})
            )
            .is_err()
        );
        assert_eq!(app.settings.raw, original);
        // A GUI draft is read from memory, not from its on-disk profile.
        app.plans[2].interval_s = 0.321;
        assert_eq!(
            bridge
                .handle(&mut app, &json!({"action":"config"}))
                .unwrap()["profile"]["measurements"]["srkr"]["wait_s"],
            json!(0.321)
        );
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
        call(&mut bridge, &mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"workflow":"srkr","plan":plan,"output":dir.path().join("srkr.csv")})).unwrap();
        finish(&mut app, &mut bridge);
        for workflow in Workflow::ALL {
            let view = call(
                &mut bridge,
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
        call(&mut bridge, &mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"workflow":"signal_monitor","output":dir.path().join("monitor.csv")})).unwrap();
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
            call(
                &mut bridge,
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
        let receipt = call(&mut bridge, &mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":dir.path().join("plot.csv")})).unwrap();
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
            call(
                &mut bridge,
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
        let receipt = call(&mut bridge, &mut app, &request).unwrap();
        assert!(app.snapshot.running);
        assert_eq!(call(&mut bridge, &mut app, &request).unwrap(), receipt);
        let other = json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string()});
        assert!(
            call(&mut bridge, &mut app, &other)
                .unwrap_err()
                .contains("busy")
        );
        finish(&mut app, &mut bridge);
        let result = bridge.run(receipt["id"].as_str().unwrap()).unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["rows"], 3);
        assert!(output.exists());
        assert!(output.with_extension("run.json").exists());
        assert_eq!(call(&mut bridge, &mut app, &request).unwrap(), result);
        assert!(!app.snapshot.running);
    }
    #[test]
    fn stop_and_failed_output_are_reported_without_affecting_another_run() {
        let (_dir, mut bridge) = bridge();
        let mut app = demo_app();
        app.plans[0].n_points = 100;
        app.plans[0].interval_s = 1.;
        let dir = tempfile::tempdir().unwrap();
        let receipt = call(&mut bridge, &mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":dir.path().join("stop.csv")})).unwrap();
        assert!(
            call(
                &mut bridge,
                &mut app,
                &json!({"action":"stop","run_id":"wrong"})
            )
            .is_err()
        );
        // The GUI Stop button uses the same cancellation token.
        app.controller.stop();
        finish(&mut app, &mut bridge);
        assert_eq!(
            bridge.run(receipt["id"].as_str().unwrap()).unwrap()["status"],
            "stopped"
        );
        assert_eq!(
            call(
                &mut bridge,
                &mut app,
                &json!({"action":"stop","run_id":receipt["id"]})
            )
            .unwrap()["status"],
            "stopped"
        );
        let read = call(
            &mut bridge,
            &mut app,
            &json!({"action":"read","request_id":uuid::Uuid::new_v4().to_string()}),
        )
        .unwrap();
        finish(&mut app, &mut bridge);
        assert_eq!(
            bridge.run(read["id"].as_str().unwrap()).unwrap()["status"],
            "completed"
        );

        let output = dir.path().join("existing.csv");
        std::fs::write(&output, "preserve").unwrap();
        let failed = call(&mut bridge, &mut app, &json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string(),"output":output})).unwrap();
        finish(&mut app, &mut bridge);
        assert_eq!(
            bridge.run(failed["id"].as_str().unwrap()).unwrap()["status"],
            "error"
        );
        assert_eq!(
            bridge.run(failed["id"].as_str().unwrap()).unwrap()["error_code"],
            "save_failed"
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
        assert!(call(&mut bridge, &mut app, &request).is_err());
        assert!(!app.snapshot.running);
        assert_eq!(app.plans[0].n_points, 3);
        app.source_demo = false;
        let request = json!({"action":"start","request_id":uuid::Uuid::new_v4().to_string()});
        assert!(
            call(&mut bridge, &mut app, &request)
                .unwrap_err()
                .contains("Connect")
        );
        assert!(!app.snapshot.running);
    }
}
