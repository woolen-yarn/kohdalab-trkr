#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! A deliberately narrow Windows entry point for previewing recorded CSV data.
//!
//! It never forwards flags to the measurement application: the only accepted
//! optional argument is one path supplied by Explorer drag-and-drop.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(not(windows))]
use std::io;

#[cfg(windows)]
const APPLICATION: &str = "kohdalab-trkr.exe";
#[cfg(not(windows))]
const APPLICATION: &str = "kohdalab-trkr";

fn application_path(launcher: &Path) -> PathBuf {
    launcher
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(APPLICATION)
}

fn main() {
    if let Err(error) = run(env::args_os().skip(1).collect()) {
        show_error(&error);
        std::process::exit(1);
    }
}

fn run(arguments: Vec<std::ffi::OsString>) -> Result<(), String> {
    if arguments.len() > 1 {
        return Err("Drag one CSV file onto KohdaLab TRKR Demo, or open it by itself.".into());
    }
    let launcher = env::current_exe().map_err(|error| format!("Find demo launcher: {error}"))?;
    let application = application_path(&launcher);
    if !application.is_file() {
        return Err(format!(
            "Missing {} next to KohdaLab TRKR Demo. Re-extract or reinstall the complete package.",
            APPLICATION
        ));
    }
    let working_directory = application.parent().unwrap_or_else(|| Path::new("."));
    let mut command = Command::new(&application);
    command.current_dir(working_directory).arg("--demo");
    if let Some(path) = arguments.first() {
        let path = std::fs::canonicalize(path).map_err(|e| format!("Open CSV: {e}"))?;
        command.arg(path);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Start {}: {error}", APPLICATION))
}

#[cfg(windows)]
fn show_error(message: &str) {
    use std::os::windows::ffi::OsStrExt;

    let wide =
        |value: &str| -> Vec<u16> { OsStr::new(value).encode_wide().chain(Some(0)).collect() };
    let title = wide("KohdaLab TRKR Demo");
    let body = wide(message);
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that live through
    // the call; null parent is the documented top-level message-box handle.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), 0x10);
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        window: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}

#[cfg(not(windows))]
fn show_error(message: &str) {
    let _ = writeln_stderr(message);
}

#[cfg(not(windows))]
fn writeln_stderr(message: &str) -> io::Result<()> {
    use std::io::Write;
    writeln!(io::stderr(), "KohdaLab TRKR Demo: {message}")
}
