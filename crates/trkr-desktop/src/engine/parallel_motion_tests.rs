//! Simulated CONEX-CC controllers for parallel-motion ownership tests. No
//! serial port is opened; every exchange records the thread that issued it.
use super::*;
use std::collections::HashMap;
use trkr_motion::{MotionConfig, ScannerConfig, protocol::Bus, shareable_bus};

pub(in crate::engine) struct SimAxis {
    /// Native position (mm) and pending target.
    pub position: f64,
    target: f64,
    pub state: String,
    polls_left: u32,
    /// TS polls before arrival; `u32::MAX` never arrives.
    pub move_polls: u32,
    /// Added to the target on arrival (readback mismatch).
    pub arrival_error: f64,
    /// After ST, every TS query fails (stationary state unconfirmable).
    pub ts_fails_after_stop: bool,
    stopped: bool,
}
impl Default for SimAxis {
    fn default() -> Self {
        Self {
            position: 6.0,
            target: 6.0,
            state: "32".into(),
            polls_left: 0,
            move_polls: 3,
            arrival_error: 0.0,
            ts_fails_after_stop: false,
            stopped: false,
        }
    }
}

#[derive(Default)]
pub(in crate::engine) struct SimState {
    pub axes: HashMap<u8, SimAxis>,
    /// (command, issuing thread name)
    pub log: Vec<(String, Option<String>)>,
}

/// One simulated serial session; clones observe the same controllers.
#[derive(Clone, Default)]
pub(in crate::engine) struct Sim(pub Arc<Mutex<SimState>>);
impl Sim {
    pub fn with_addresses(addresses: &[u8]) -> Self {
        let sim = Self::default();
        for address in addresses {
            lock(&sim.0).axes.insert(*address, SimAxis::default());
        }
        sim
    }
    pub fn commands(&self) -> Vec<String> {
        lock(&self.0).log.iter().map(|(c, _)| c.clone()).collect()
    }
    pub fn threads(&self) -> Vec<Option<String>> {
        lock(&self.0).log.iter().map(|(_, t)| t.clone()).collect()
    }
    pub fn axis<T>(&self, address: u8, f: impl FnOnce(&mut SimAxis) -> T) -> T {
        f(lock(&self.0)
            .axes
            .get_mut(&address)
            .expect("simulated address"))
    }
    pub fn bus(&self) -> Box<dyn Bus + Send> {
        shareable_bus(Box::new(SimBus(self.clone())))
    }
}

struct SimBus(Sim);
fn split(command: &str) -> (u8, &str) {
    let digits = command.chars().take_while(char::is_ascii_digit).count();
    (command[..digits].parse().unwrap_or(0), &command[digits..])
}
impl Bus for SimBus {
    fn query(&mut self, command: &str) -> Result<String, String> {
        let mut state = lock(&self.0.0);
        state
            .log
            .push((command.into(), thread::current().name().map(str::to_owned)));
        let (address, body) = split(command);
        let axis = state
            .axes
            .get_mut(&address)
            .ok_or_else(|| format!("no simulated controller at {address}"))?;
        match body {
            "TS" => {
                if axis.ts_fails_after_stop && axis.stopped {
                    return Err("simulated TS failure after STOP".into());
                }
                if axis.polls_left > 0 && axis.polls_left != u32::MAX {
                    axis.polls_left -= 1;
                    if axis.polls_left == 0 {
                        axis.position = axis.target + axis.arrival_error;
                        axis.state = "33".into();
                    }
                }
                Ok(format!("{address}TS0000{}", axis.state))
            }
            "TP" => Ok(format!("{address}TP{}", axis.position)),
            "SC?" => Ok(format!("{address}SC1")),
            _ => Err(format!("unexpected query {command}")),
        }
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        let mut state = lock(&self.0.0);
        state
            .log
            .push((command.into(), thread::current().name().map(str::to_owned)));
        let (address, body) = split(command);
        let axis = state
            .axes
            .get_mut(&address)
            .ok_or_else(|| format!("no simulated controller at {address}"))?;
        if let Some(value) = body.strip_prefix("PA") {
            axis.target = value.parse().map_err(|_| format!("bad PA {command}"))?;
            if axis.move_polls == 0 {
                axis.position = axis.target + axis.arrival_error;
                axis.state = "33".into();
            } else {
                axis.state = "28".into();
                axis.polls_left = axis.move_polls;
            }
            return Ok(());
        }
        match body {
            "ST" => {
                axis.stopped = true;
                axis.polls_left = 0;
                axis.state = "33".into();
            }
            "MM0" => axis.state = "3C".into(),
            "MM1" => axis.state = "32".into(),
            "SC1" => {}
            _ => return Err(format!("unexpected write {command}")),
        }
        Ok(())
    }
}

/// One simulated single-axis SHOT delay controller (`?:S1`, `Q:`, `A:`,
/// `G:`, `!:`, `L:1`), recording the thread that issued each exchange.
pub(in crate::engine) struct DelayState {
    pub pulse: i64,
    target: i64,
    polls_left: u32,
    /// `!:` polls before arrival; `u32::MAX` never arrives.
    pub move_polls: u32,
    pub log: Vec<(String, Option<String>)>,
}
#[derive(Clone)]
pub(in crate::engine) struct DelaySim(pub Arc<Mutex<DelayState>>);
impl Default for DelaySim {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(DelayState {
            pulse: 12_500,
            target: 12_500,
            polls_left: 0,
            move_polls: 3,
            log: Vec::new(),
        })))
    }
}
impl DelaySim {
    pub fn commands(&self) -> Vec<String> {
        lock(&self.0).log.iter().map(|(c, _)| c.clone()).collect()
    }
    pub fn log(&self) -> Vec<(String, Option<String>)> {
        lock(&self.0).log.clone()
    }
    pub fn moving(&self) -> bool {
        lock(&self.0).polls_left > 0
    }
    pub fn bus(&self) -> Box<dyn Bus + Send> {
        shareable_bus(Box::new(DelayBus(self.clone())))
    }
}
impl DelayState {
    /// A real stage moves regardless of which status query the host sends.
    /// The driver polls `Q:` and only asks `!:` once the position matches
    /// (`reached && delay_ready`), so motion must advance on both queries;
    /// advancing on `!:` alone never arrives.
    fn advance(&mut self) {
        if self.polls_left > 0 && self.polls_left != u32::MAX {
            self.polls_left -= 1;
            if self.polls_left == 0 {
                self.pulse = self.target;
            }
        }
    }
}
struct DelayBus(DelaySim);
impl Bus for DelayBus {
    fn query(&mut self, command: &str) -> Result<String, String> {
        let mut state = lock(&self.0.0);
        state
            .log
            .push((command.into(), thread::current().name().map(str::to_owned)));
        if matches!(command, "Q:" | "!:") {
            state.advance();
        }
        if let Some(rest) = command.strip_prefix("A:1") {
            let (sign, magnitude) = rest.split_at(1);
            let magnitude: i64 = magnitude
                .strip_prefix('P')
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| format!("bad A {command}"))?;
            state.target = if sign == "-" { -magnitude } else { magnitude };
            return Ok("OK".into());
        }
        Ok(match command {
            "?:S1" => "1".into(),
            "Q:" => state.pulse.to_string(),
            "G:" => {
                state.polls_left = state.move_polls;
                if state.polls_left == 0 {
                    state.pulse = state.target;
                }
                "OK".into()
            }
            "!:" => (if state.polls_left == 0 { "R" } else { "B" }).into(),
            "L:1" => {
                state.polls_left = 0;
                "OK".into()
            }
            _ => return Err(format!("unexpected delay query {command}")),
        })
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        Err(format!("unexpected delay write {command}"))
    }
}

pub(in crate::engine) fn delay_config(port: &str) -> trkr_motion::DelayConfig {
    trkr_motion::DelayConfig {
        port: port.into(),
        stage: Some("SGSP46-500".into()),
        direction: 1,
        ..Default::default()
    }
}

/// Opens a delay-only rig through the production constructor.
pub(in crate::engine) fn delay_rig(config: &MotionConfig, sim: &DelaySim) -> MotionRig {
    MotionRig::open_with_bus_factory(config, |_, _, _, _, _, _| Ok(sim.bus())).unwrap()
}

pub(in crate::engine) fn cc(port: &str, address: u8) -> ScannerConfig {
    ScannerConfig {
        port: port.into(),
        controller: "CONEXCC".into(),
        actuator: Some("TRA12CC".into()),
        controller_address: address,
        sample_um_per_unit: 1.0,
        ..Default::default()
    }
}

/// Opens X/Y through the production rig constructor, routing each port name
/// to its simulated session.
pub(in crate::engine) fn scanner_rig(config: &MotionConfig, sims: &[(&str, &Sim)]) -> MotionRig {
    MotionRig::open_with_bus_factory(config, |port, _, _, _, _, _| {
        sims.iter()
            .find(|(name, _)| *name == port)
            .map(|(_, sim)| sim.bus())
            .ok_or_else(|| format!("no simulated port {port}"))
    })
    .unwrap()
}

fn two_port_rig(x: &Sim, y: &Sim) -> MotionRig {
    let config = MotionConfig {
        x: Some(cc("COM5", 1)),
        y: Some(cc("COM4", 1)),
        ..Default::default()
    };
    scanner_rig(&config, &[("COM5", x), ("COM4", y)])
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn x_move(target_um: f64) -> ParallelMove {
    ParallelMove {
        targets: [None, Some(target_um), None],
        approach: [false, true, true],
    }
}

#[test]
fn requests_without_a_running_owner_are_refused() {
    let slot = Slot::default();
    assert!(slot.status().is_none());
    assert!(slot.request(x_move(1.0)).is_err());
    slot.abort();
}

#[test]
fn request_validation_rejects_unowned_axes_and_overlapping_moves() {
    let status = ParallelMotionStatus {
        axes: [false, true, false],
        accepting: true,
        ..Default::default()
    };
    assert!(validate_request(&status, &x_move(1.0)).is_ok());
    let delay = ParallelMove {
        targets: [Some(1.0), None, None],
        approach: [false; 3],
    };
    let y = ParallelMove {
        targets: [None, None, Some(1.0)],
        approach: [false, true, true],
    };
    for request in [delay, y, x_move(f64::NAN), x_move(f64::INFINITY)] {
        assert!(validate_request(&status, &request).is_err(), "{request:?}");
    }
    let none = ParallelMove {
        targets: [None; 3],
        approach: [false; 3],
    };
    assert!(validate_request(&status, &none).is_err());
    for refused in [
        ParallelMotionStatus {
            moving: true,
            ..status.clone()
        },
        ParallelMotionStatus {
            fault: Some("earlier failure".into()),
            ..status.clone()
        },
        ParallelMotionStatus {
            accepting: false,
            ..status.clone()
        },
    ] {
        assert!(validate_request(&refused, &x_move(1.0)).is_err());
    }
}

#[test]
fn executor_owns_only_its_connection_and_moves_off_the_calling_thread() {
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    let rig = two_port_rig(&x, &y);
    let (mut remaining, owned) = rig.split_exclusive([false, true, false]).unwrap();
    // The original view can no longer reach X; Y is still shared.
    let mut original = rig;
    assert!(original.position().is_err());
    assert_eq!(remaining.connected_axes(), [false, false, true]);
    remaining.position().unwrap();

    let slot = Slot::default();
    let global = Arc::new(AtomicBool::new(false));
    let executor = Executor::spawn(&slot, owned, global, Instant::now()).unwrap();
    wait_for("initial readback", || {
        slot.status()
            .is_some_and(|s| s.position.physical[1].is_some())
    });
    let before = x.commands().len();
    slot.request(x_move(2.0)).unwrap();
    // One move at a time: a second request is refused, not queued.
    assert!(slot.request(x_move(3.0)).is_err());
    wait_for("move", || slot.status().is_some_and(|s| s.finished == 1));
    let status = slot.status().unwrap();
    assert!(!status.moving && status.fault.is_none(), "{status:?}");
    assert_eq!(status.position.physical[1], Some(2.0));
    // Stationary check precedes the command; readback follows it.
    let commands = x.commands()[before..].to_vec();
    let pa = commands.iter().position(|c| c.starts_with("1PA")).unwrap();
    assert_eq!(commands[0], "1TS");
    assert!(commands[pa + 1..].iter().any(|c| c == "1TP"));
    // Every X exchange came from the executor thread.
    for thread in &x.threads()[before..] {
        assert_eq!(thread.as_deref(), Some("trkr-parallel-motion"));
    }
    assert!(y.commands().iter().all(|c| !c.contains("PA")));

    let finished = executor.finish(false);
    assert!(finished.error.is_none());
    assert!(
        finished
            .audits
            .iter()
            .any(|a| a["event"] == "parallel_move" && a["status"] == "complete")
    );
    assert!(slot.status().is_none());
    // Joining released the claim: the shared view works again.
    original.position().unwrap();
}

#[test]
fn rejected_target_performs_no_io_and_does_not_latch_a_fault() {
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    let (_remaining, owned) = two_port_rig(&x, &y)
        .split_exclusive([false, true, false])
        .unwrap();
    let slot = Slot::default();
    let executor = Executor::spawn(
        &slot,
        owned,
        Arc::new(AtomicBool::new(false)),
        Instant::now(),
    )
    .unwrap();
    wait_for("initial readback", || {
        slot.status()
            .is_some_and(|s| s.position.physical[1].is_some())
    });
    let before = x.commands().len();
    // Outside the TRA12CC travel limits.
    slot.request(x_move(100.0)).unwrap();
    wait_for("rejection", || {
        slot.status().is_some_and(|s| s.finished == 1)
    });
    let status = slot.status().unwrap();
    assert!(
        status.fault.is_none() && status.last_error.is_some(),
        "{status:?}"
    );
    assert_eq!(x.commands().len(), before);
    slot.request(x_move(1.0)).unwrap();
    wait_for("move", || slot.status().is_some_and(|s| s.finished == 2));
    assert!(executor.finish(false).error.is_none());
}

#[test]
fn failed_arrival_stops_confirms_stationary_and_refuses_the_next_move() {
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    x.axis(1, |a| a.arrival_error = 0.5);
    let (_remaining, owned) = two_port_rig(&x, &y)
        .split_exclusive([false, true, false])
        .unwrap();
    let slot = Slot::default();
    let executor = Executor::spawn(
        &slot,
        owned,
        Arc::new(AtomicBool::new(false)),
        Instant::now(),
    )
    .unwrap();
    slot.request(x_move(2.0)).unwrap();
    wait_for("failure", || slot.status().is_some_and(|s| s.finished == 1));
    let status = slot.status().unwrap();
    assert!(status.fault.is_some() && !status.unconfirmed, "{status:?}");
    assert!(x.commands().iter().any(|c| c == "1ST"));
    let before = x.commands().len();
    assert!(slot.request(x_move(1.0)).is_err());
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        x.commands().len(),
        before,
        "a refused move must not reach the controller"
    );
    // A confirmed-stationary failure does not by itself fail the monitor.
    assert!(executor.finish(false).error.is_none());
}

#[test]
fn unconfirmable_stationary_state_fails_the_run() {
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    x.axis(1, |a| {
        a.arrival_error = 0.5;
        a.ts_fails_after_stop = true;
    });
    let (_remaining, owned) = two_port_rig(&x, &y)
        .split_exclusive([false, true, false])
        .unwrap();
    let slot = Slot::default();
    let executor = Executor::spawn(
        &slot,
        owned,
        Arc::new(AtomicBool::new(false)),
        Instant::now(),
    )
    .unwrap();
    slot.request(x_move(2.0)).unwrap();
    wait_for("failure", || slot.status().is_some_and(|s| s.finished == 1));
    assert!(executor.unconfirmed());
    let error = executor
        .finish(false)
        .error
        .expect("unconfirmed must fail the run");
    assert!(error.starts_with(UNCONFIRMED), "{error}");
}

#[test]
fn global_stop_aborts_an_in_flight_move_and_sends_stop() {
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    x.axis(1, |a| a.move_polls = u32::MAX);
    let (_remaining, owned) = two_port_rig(&x, &y)
        .split_exclusive([false, true, false])
        .unwrap();
    let slot = Slot::default();
    let global = Arc::new(AtomicBool::new(false));
    let executor = Executor::spawn(&slot, owned, global.clone(), Instant::now()).unwrap();
    slot.request(x_move(2.0)).unwrap();
    wait_for("motion start", || x.axis(1, |a| a.state == "28"));
    // Controller::stop sets the token and aborts the published executor.
    global.store(true, Ordering::Release);
    slot.abort();
    wait_for("abort", || slot.status().is_some_and(|s| s.finished == 1));
    let status = slot.status().unwrap();
    assert!(status.fault.is_some() && !status.unconfirmed, "{status:?}");
    let commands = x.commands();
    let pa = commands.iter().position(|c| c.starts_with("1PA")).unwrap();
    assert!(commands[pa..].iter().any(|c| c == "1ST"));
    assert!(slot.request(x_move(1.0)).is_err());
    let finished = executor.finish(true);
    assert!(finished.error.is_none());
    assert!(
        finished
            .audits
            .iter()
            .any(|a| a["status"] == "failed" && a["error"] == "motion cancelled")
    );
}

#[test]
fn dropping_an_executor_joins_it_and_releases_ownership() {
    let (x, y) = (Sim::with_addresses(&[1]), Sim::with_addresses(&[1]));
    let mut rig = two_port_rig(&x, &y);
    let (_remaining, owned) = rig.split_exclusive([false, true, true]).unwrap();
    let slot = Slot::default();
    let executor = Executor::spawn(
        &slot,
        owned,
        Arc::new(AtomicBool::new(false)),
        Instant::now(),
    )
    .unwrap();
    assert!(rig.position().is_err());
    drop(executor);
    assert!(slot.status().is_none());
    rig.position().unwrap();
}

fn delay_only(sim: &DelaySim) -> MotionRig {
    let config = MotionConfig {
        delay: Some(delay_config("COM6")),
        ..Default::default()
    };
    delay_rig(&config, sim)
}

/// Regression: the simulated delay stage must arrive under the production
/// wait loop, which polls `Q:` and asks `!:` only once the position matches.
/// Bounded: on timeout the move is cancelled so the worker exits instead of
/// running into the 240 s delay timeout.
#[test]
fn delay_sim_arrives_under_the_production_wait_loop_within_a_bound() {
    let sim = DelaySim::default();
    let mut rig = delay_only(&sim);
    let target = rig.position().unwrap().physical[0].unwrap() + 10.0;
    let cancel = Arc::new(AtomicBool::new(false));
    let (done, finished) = mpsc::channel();
    let worker = {
        let cancel = cancel.clone();
        thread::spawn(move || {
            let _ = done.send(rig.move_to([Some(target), None, None], [false; 3], &cancel));
        })
    };
    let result = finished.recv_timeout(Duration::from_secs(5));
    cancel.store(true, Ordering::Release);
    worker.join().unwrap();
    let position = result
        .expect("simulated delay move must finish without relying on the driver timeout")
        .unwrap();
    assert!((position.physical[0].unwrap() - target).abs() < 0.5);
    let commands = sim.commands();
    let go = commands.iter().position(|c| c == "G:").unwrap();
    assert!(commands[go..].iter().any(|c| c == "Q:"));
    assert_eq!(commands.last().map(String::as_str), Some("Q:"));
}

#[test]
fn executor_owns_the_delay_stage_confirms_ready_before_moving_and_stops_it() {
    let sim = DelaySim::default();
    let rig = delay_only(&sim);
    let (remaining, owned) = rig.split_exclusive([true, false, false]).unwrap();
    assert_eq!(remaining.connected_axes(), [false; 3]);
    let slot = Slot::default();
    let global = Arc::new(AtomicBool::new(false));
    let executor = Executor::spawn(&slot, owned, global.clone(), Instant::now()).unwrap();
    wait_for("initial readback", || {
        slot.status()
            .is_some_and(|s| s.position.physical[0].is_some())
    });
    let status = slot.status().unwrap();
    assert_eq!(status.axes, [true, false, false]);
    let start_ps = status.position.physical[0].unwrap();
    let before = sim.commands().len();
    let delay_move = |ps: f64| ParallelMove {
        targets: [Some(ps), None, None],
        approach: [false, true, true],
    };
    slot.request(delay_move(start_ps + 10.0)).unwrap();
    wait_for("delay move", || {
        slot.status().is_some_and(|s| s.finished == 1)
    });
    let status = slot.status().unwrap();
    assert!(status.fault.is_none(), "{status:?}");
    assert_ne!(status.position.delay_stage_pulse, Some(12_500));
    assert!((status.position.physical[0].unwrap() - (start_ps + 10.0)).abs() < 0.5);
    // The stationary (READY) check precedes the move command.
    let commands = sim.commands()[before..].to_vec();
    let started = commands.iter().position(|c| c.starts_with("A:1")).unwrap();
    assert_eq!(commands[0], "!:");
    assert!(started > 0);

    // Global Stop during a move that never arrives sends the delay STOP and
    // confirms READY independently; later moves are refused.
    lock(&sim.0).move_polls = u32::MAX;
    slot.request(delay_move(start_ps)).unwrap();
    wait_for("delay motion start", || sim.moving());
    global.store(true, Ordering::Release);
    slot.abort();
    wait_for("abort", || slot.status().is_some_and(|s| s.finished == 2));
    let status = slot.status().unwrap();
    assert!(status.fault.is_some() && !status.unconfirmed, "{status:?}");
    let commands = sim.commands();
    let go = commands.iter().rposition(|c| c == "G:").unwrap();
    let stop = go + commands[go..].iter().position(|c| c == "L:1").unwrap();
    assert!(commands[stop..].iter().any(|c| c == "!:"));
    assert!(slot.request(delay_move(start_ps)).is_err());
    for (_, thread) in &sim.log()[before..] {
        assert_eq!(thread.as_deref(), Some("trkr-parallel-motion"));
    }
    assert!(executor.finish(true).error.is_none());
}
