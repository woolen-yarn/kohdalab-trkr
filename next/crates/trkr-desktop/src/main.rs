#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod axis_choices;
mod config;
mod engine;
mod file_dialog;
mod live;
mod live_view;
mod lockin;
mod plot_history;
mod precision;
mod presentation;
#[cfg(test)]
mod python_parity_tests;
mod replay_data;
mod replay_player;
mod ui;
use std::io::{BufRead, Read, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag] if flag == "--stdio" => stdio(),
        [flag] if flag == "--gpib-resources" => trkr_transport::available_gpib_resources()
            .map(|resources| println!("{}", serde_json::json!({"resources":resources}))),
        [flag] if flag == "--demo" => run_demo(None, None),
        [flag, path] if flag == "--demo" => run_demo(Some(path.into()), None),
        [flag, dir] if flag == "--gui-smoke" => gui_smoke(dir.into()),
        [flag, dir, width, height] if flag == "--gui-layout-smoke" => {
            layout_capture(dir.into(), width, height, "")
        }
        [flag, dir, width, height, state] if flag == "--gui-layout-smoke" => {
            layout_capture(dir.into(), width, height, state)
        }
        [flag, output, data] if flag == "--demo-smoke" => {
            let output = std::path::PathBuf::from(output);
            if output.exists() {
                Err("Demo smoke output already exists".into())
            } else {
                std::fs::create_dir_all(&output)
                    .map_err(|e| e.to_string())
                    .and_then(|_| run_demo(Some(data.into()), Some(output)))
            }
        }
        [flag, path] if flag == "--profile-json" => profile_json(std::path::Path::new(path)),
        _ if args.is_empty() => run_gui(None),
        _ => cli(&args),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1)
    }
    Ok(())
}
fn gui_smoke(dir: std::path::PathBuf) -> Result<(), String> {
    if dir.exists() {
        return Err(format!(
            "Smoke output directory already exists: {}",
            dir.display()
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ok = Arc::new(AtomicBool::new(false));
    run_gui(Some((dir, ok.clone())))?;
    if ok.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("GUI smoke test did not complete successfully".into())
    }
}
fn layout_capture(
    dir: std::path::PathBuf,
    width: &str,
    height: &str,
    state: &str,
) -> Result<(), String> {
    if ![
        "",
        "devices-expanded",
        "right-collapsed",
        "left-collapsed",
        "devices-scrolled",
        "rotated",
        "rotated-reverse",
    ]
    .contains(&state)
    {
        return Err("Unknown layout state".into());
    }
    let w: f32 = width.parse().map_err(|_| "Invalid layout width")?;
    let h: f32 = height.parse().map_err(|_| "Invalid layout height")?;
    if !(1143.0..=7680.0).contains(&w) || !(700.0..=4320.0).contains(&h) {
        return Err("Layout dimensions out of range".into());
    }
    if dir.exists() {
        return Err("Layout output already exists".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let success = Arc::new(AtomicBool::new(false));
    let width = w.max(precision::Columns::minimum_width(
        state == "right-collapsed",
        state == "left-collapsed",
    ));
    run_gui_at(Some((dir, success.clone())), [width, h], true, state)?;
    if success.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("Layout capture did not finish".into())
    }
}
fn run_gui(smoke: Option<(std::path::PathBuf, Arc<AtomicBool>)>) -> Result<(), String> {
    run_gui_at(smoke, [1512., 900.], false, "")
}
fn run_gui_at(
    smoke: Option<(std::path::PathBuf, Arc<AtomicBool>)>,
    size: [f32; 2],
    layout_only: bool,
    state: &str,
) -> Result<(), String> {
    eframe::run_native(
        &format!("KohdaLab TRKR v{}", env!("CARGO_PKG_VERSION")),
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_icon(app_icon())
                .with_inner_size(size)
                .with_min_inner_size([
                    precision::Columns::minimum_width(
                        state == "right-collapsed",
                        state == "left-collapsed",
                    ),
                    700.,
                ]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = ui::App::new(cc, false);
            if let Some((dir, ok)) = smoke {
                if layout_only {
                    app.enable_layout_capture(dir, ok, state)
                } else {
                    app.enable_smoke(dir, ok)
                }
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}
fn stdio() -> Result<(), String> {
    const MAX: usize = 1024 * 1024;
    let (tx, rx) = mpsc::channel::<Result<engine::Command, String>>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            let mut bytes = Vec::new();
            match input
                .by_ref()
                .take((MAX + 1) as u64)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) => break,
                Ok(_) if bytes.len() > MAX => {
                    let _ = tx.send(Err("JSON command exceeds 1 MiB".into()));
                    break;
                }
                Ok(_) => {
                    let parsed = String::from_utf8(bytes)
                        .map_err(|_| "JSON command must be UTF-8".into())
                        .and_then(|line| {
                            serde_json::from_str(line.trim())
                                .map_err(|e| format!("Invalid JSON command: {e}"))
                        });
                    if tx.send(parsed).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("Read stdin: {e}")));
                    break;
                }
            }
        }
    });
    let control = engine::controller();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut eof = false;
    loop {
        while let Ok(event) = control.events.try_recv() {
            emit(&mut out, &event)?
        }
        if eof {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(command)) => {
                if let Err(e) = control.send(command) {
                    emit_rejected(&mut out, e)?
                }
            }
            Ok(Err(e)) => emit_rejected(&mut out, e)?,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                control.stop();
                eof = true
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    while let Ok(event) = control.events.try_recv() {
        emit(&mut out, &event)?
    }
    Ok(())
}
fn emit(out: &mut impl Write, event: &engine::Event) -> Result<(), String> {
    serde_json::to_writer(&mut *out, event).map_err(|e| e.to_string())?;
    out.write_all(b"\n").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())
}
fn emit_rejected(out: &mut impl Write, error: String) -> Result<(), String> {
    serde_json::to_writer(
        &mut *out,
        &serde_json::json!({"event":"rejected","data":error}),
    )
    .map_err(|e| e.to_string())?;
    out.write_all(b"\n").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())
}
fn profile_json(path: &std::path::Path) -> Result<(), String> {
    let settings = config::Settings::load(path)?;
    let mut plans = serde_json::Map::new();
    for workflow in trkr_core::Workflow::ALL {
        plans.insert(
            workflow.key().into(),
            serde_json::to_value(settings.plan(workflow)?).map_err(|e| e.to_string())?,
        );
    }
    println!(
        "{}",
        serde_json::json!({"hardware":settings.hardware()?,"plans":plans})
    );
    Ok(())
}
fn cli(args: &[String]) -> Result<(), String> {
    if args == ["--help"] || args == ["-h"] {
        println!(
            "KohdaLab TRKR\n--stdio JSON-lines controller API\n--gui-smoke NEW_DIRECTORY\n--gui-layout-smoke NEW_DIRECTORY WIDTH HEIGHT [devices-expanded|right-collapsed|rotated]\n--profile-json FILE\n--demo-csv FILE [--workflow signal_monitor|trkr|srkr|strkr|srkr_2d]"
        );
        return Ok(());
    }
    if !(args.len() == 2 || args.len() == 4) || args[0] != "--demo-csv" {
        return Err("Use --help for supported arguments".into());
    }
    let mut plan = trkr_core::Plan::default();
    if args.len() == 4 {
        if args[2] != "--workflow" {
            return Err("Expected --workflow".into());
        }
        plan.workflow = trkr_core::Workflow::ALL
            .into_iter()
            .find(|w| w.key() == args[3])
            .ok_or("Unknown workflow")?
    }
    match plan.workflow {
        trkr_core::Workflow::Srkr => plan.fast = trkr_core::Axis::X,
        trkr_core::Workflow::Srkr2d => {
            plan.fast = trkr_core::Axis::X;
            plan.slow = trkr_core::Axis::Y
        }
        _ => {}
    }
    let n = engine::demo_csv(plan, std::path::Path::new(&args[1]))?;
    println!("Saved {n} simulated points to {}", args[1]);
    Ok(())
}

fn run_demo(
    path: Option<std::path::PathBuf>,
    smoke: Option<std::path::PathBuf>,
) -> Result<(), String> {
    eframe::run_native(
        "KohdaLab TRKR — DEMO MODE — NO HARDWARE",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_icon(app_icon())
                .with_inner_size([1512., 900.])
                .with_min_inner_size([precision::Columns::minimum_width(false, false), 700.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = ui::App::new(cc, true);
            app.open_demo(path);
            if let Some(output) = smoke {
                app.enable_demo_smoke(output);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

fn app_icon() -> Arc<eframe::egui::IconData> {
    Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("embedded TRKR icon"),
    )
}
