//! Small, process-isolated GPIB client for the KohdaLab helper protocol.
//!
//! Instrument/model logic deliberately lives above this crate.  The helper is
//! an independently licensed executable; this crate only owns framing,
//! deadlines, and the adapter lease.
//!
//! Development shared mode is selected only by KOHDALAB_INSTRUMENT_HUB_CONFIG.
//! The authenticated hub proxy owns the bus; errors never fall back to a
//! local native/VISA helper. See docs/development/shared-instruments.md.

use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const VISA_LIST_TIMEOUT: Duration = Duration::from_secs(8);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: usize = 256 * 1024;
const MAX_PAYLOAD: usize = 128 * 1024;

/// Fixed transport failures.  This remains private because callers retain the
/// established `Result<_, String>` interface, while keeping each failure
/// category explicit inside the process boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportError {
    NativeHelperNotFound,
    VisaHelperNotFound,
    HelperStdinUnavailable,
    HelperStdoutUnavailable,
    HelperFrameExceededSizeLimit,
    ReadCancelled,
    ResponsePending,
    SessionFailed,
    CommandTooLong,
    ResponseTimedOut,
    HelperExited,
    VisaHelperStdoutUnavailable,
    VisaDiscoveryNoBoundedFrame,
    VisaDiscoveryTimedOut,
    VisaDiscoveryExited,
    InvalidVisaResourceList,
    InvalidAddress,
    InvalidHexPayload,
    MalformedHelperFrame,
    MalformedHelperStatus,
    MalformedHelperPayload,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NativeHelperNotFound => {
                "KohdaLab GPIB helper was not found; set KOHDALAB_TRKR_HELPER_DIR"
            }
            Self::VisaHelperNotFound => {
                "Official VISA helper was not found; GPIB Refresh requires kohdalab-visa-helper"
            }
            Self::HelperStdinUnavailable => "GPIB helper stdin was unavailable",
            Self::HelperStdoutUnavailable => "GPIB helper stdout was unavailable",
            Self::HelperFrameExceededSizeLimit => "GPIB helper frame exceeded size limit",
            Self::ReadCancelled => "GPIB read cancelled",
            Self::ResponsePending => {
                "GPIB cancelled reply pending; finish cancellation before further I/O"
            }
            Self::SessionFailed => "GPIB session has failed; reconnect before further I/O",
            Self::CommandTooLong => "GPIB command exceeds 4096-byte limit",
            Self::ResponseTimedOut => "GPIB helper response timed out",
            Self::HelperExited => "GPIB helper exited unexpectedly",
            Self::VisaHelperStdoutUnavailable => "Official VISA helper stdout was unavailable",
            Self::VisaDiscoveryNoBoundedFrame => {
                "Official VISA helper discovery returned no bounded KIV frame"
            }
            Self::VisaDiscoveryTimedOut => "Official VISA helper discovery timed out",
            Self::VisaDiscoveryExited => "Official VISA helper discovery exited unexpectedly",
            Self::InvalidVisaResourceList => {
                "Official VISA helper returned an invalid resource list"
            }
            Self::InvalidAddress => "GPIB address must be between 1 and 30",
            Self::InvalidHexPayload => "Invalid GPIB helper hex payload",
            Self::MalformedHelperFrame => "Malformed GPIB helper frame",
            Self::MalformedHelperStatus => "Malformed GPIB helper status",
            Self::MalformedHelperPayload => "Malformed GPIB helper payload",
        };
        formatter.write_str(message)
    }
}

impl From<TransportError> for String {
    fn from(error: TransportError) -> Self {
        error.to_string()
    }
}

pub struct Gpib {
    child: Child,
    input: std::process::ChildStdin,
    lines: Receiver<Result<String, String>>,
    // The direct helper owns this compatibility lease. The opt-in instrument
    // hub owns it instead, so clients may share one controller safely.
    _lease: Option<File>,
    address: u8,
    backend: &'static str,
    poisoned: bool,
    cancelled_reply_deadline: Option<Instant>,
}

impl Gpib {
    pub fn open(helper: impl AsRef<Path>, address: u8) -> Result<Self, String> {
        validate_address(address)?;
        let helper = helper.as_ref();
        if let Some(config) = hub_config()? {
            return Self::spawn_hub_proxy(address, &config, Some(helper));
        }
        if !helper.is_file() {
            return Err(format!("GPIB helper was not found: {}", helper.display()));
        }
        Self::spawn_helper(helper, address, &[], "native")
    }

    /// Connect to the requested primary GPIB address.  When the optional
    /// official VISA helper exposes that exact endpoint, VISA owns the whole
    /// session.  In particular, an error from that selected session is never
    /// retried through the native helper, since opening the latter can reset a
    /// shared GPIB bus.
    pub fn connect(address: u8) -> Result<Self, String> {
        let (native, visa) = helper_paths();
        Self::connect_with_paths(address, native, visa)
    }

    /// Identifies the process transport selected by [`connect`].
    pub fn backend_name(&self) -> &'static str {
        self.backend
    }

    fn connect_with_paths(
        address: u8,
        native: Option<PathBuf>,
        visa: Option<PathBuf>,
    ) -> Result<Self, String> {
        validate_address(address)?;
        if let Some(config) = hub_config()? {
            // Do this before VISA discovery. Once the user selects the shared
            // service, no local helper or vendor runtime may open the bus.
            return Self::spawn_hub_proxy(address, &config, native.as_deref());
        }
        if let Some(visa_helper) = visa {
            let resources = visa_resources(&visa_helper)?;
            let requested = format!("GPIB0::{address}::INSTR");
            let mut other_gpib = Vec::new();
            for resource in resources {
                if resource.eq_ignore_ascii_case(&requested) {
                    // The VISA helper uses wire address zero because its
                    // process is already bound to one VISA resource.
                    return Self::spawn_helper(&visa_helper, 0, &[resource], "official VISA");
                }
                if is_gpib_resource(&resource) {
                    other_gpib.push(resource);
                }
            }
            if !other_gpib.is_empty() {
                return Err(format!(
                    "Official VISA found GPIB resource(s) {}, but not {requested}; refusing to open the native GPIB adapter",
                    other_gpib.join(", ")
                ));
            }
        }
        let native = native.ok_or_else(|| String::from(TransportError::NativeHelperNotFound))?;
        Self::open(native, address)
    }

    fn spawn_helper(
        helper: &Path,
        address: u8,
        args: &[String],
        backend: &'static str,
    ) -> Result<Self, String> {
        if !helper.is_file() {
            return Err(format!("GPIB helper was not found: {}", helper.display()));
        }
        let lease = Some(acquire_lease()?);
        Self::spawn_process(helper, address, args, backend, lease)
    }

    fn spawn_hub_proxy(address: u8, config: &Path, native: Option<&Path>) -> Result<Self, String> {
        let hub = hub_client(native)?;
        let args = vec![
            "--client".to_owned(),
            "--config".to_owned(),
            config.display().to_string(),
            "--app".to_owned(),
            "KohdaLab TRKR".to_owned(),
            "--address".to_owned(),
            address.to_string(),
        ];
        Self::spawn_process(&hub, address, &args, "instrument hub", None)
    }

    fn spawn_process(
        helper: &Path,
        address: u8,
        args: &[String],
        backend: &'static str,
        lease: Option<File>,
    ) -> Result<Self, String> {
        let mut command = Command::new(helper);
        command.args(args);
        suppress_console_window(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Start GPIB helper: {e}"))?;
        let input = child.stdin.take().ok_or_else(|| {
            let _ = child.kill();
            String::from(TransportError::HelperStdinUnavailable)
        })?;
        let output = child.stdout.take().ok_or_else(|| {
            let _ = child.kill();
            String::from(TransportError::HelperStdoutUnavailable)
        })?;
        let (sender, lines): (SyncSender<Result<String, String>>, Receiver<_>) =
            mpsc::sync_channel(4);
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                match reader
                    .by_ref()
                    .take((MAX_FRAME + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => break,
                    Ok(n) if n <= MAX_FRAME && line.last() == Some(&b'\n') => {
                        let text = String::from_utf8_lossy(&line[..n - 1]).into_owned();
                        if sender.send(Ok(text)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {
                        let _ =
                            sender.send(Err(TransportError::HelperFrameExceededSizeLimit.into()));
                        break;
                    }
                    Err(e) => {
                        let _ = sender.send(Err(format!("Read GPIB helper: {e}")));
                        break;
                    }
                }
            }
        });
        let gpib = Self {
            child,
            input,
            lines,
            _lease: lease,
            address,
            backend,
            poisoned: false,
            cancelled_reply_deadline: None,
        };
        let startup = gpib.receive(STARTUP_TIMEOUT)?;
        let (status, payload) = parse_frame(&startup)?;
        if status != "OK" || !payload.is_empty() {
            return Err(format!("GPIB helper startup failed: {startup}"));
        }
        Ok(gpib)
    }

    pub fn write(&mut self, command: &str) -> Result<(), String> {
        self.request("WRITE", command.as_bytes()).map(|_| ())
    }

    pub fn query(&mut self, command: &str) -> Result<String, String> {
        let bytes = self.request("QUERY", command.as_bytes())?;
        String::from_utf8(bytes).map_err(|e| format!("GPIB response was not UTF-8: {e}"))
    }

    /// Return promptly on Stop while retaining the in-flight reply for recovery.
    /// No further commands are allowed until that exact reply has been retired.
    pub fn write_cancellable(&mut self, command: &str, cancel: &AtomicBool) -> Result<(), String> {
        self.request_cancellable("WRITE", command.as_bytes(), Some(cancel))
            .map(|_| ())
    }
    pub fn query_cancellable(
        &mut self,
        command: &str,
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        let bytes = self.request_cancellable("QUERY", command.as_bytes(), Some(cancel))?;
        String::from_utf8(bytes).map_err(|e| format!("GPIB response was not UTF-8: {e}"))
    }
    /// Prevent further I/O when an interrupted device protocol left state uncertain.
    pub fn invalidate(&mut self) {
        self.poisoned = true;
        let _ = self.child.kill();
    }
    /// Retire exactly one cancelled helper reply after motion has stopped.
    /// A healthy late response is discarded, never used as a subsequent value.
    /// Real transport/protocol failures still invalidate the session.
    pub fn finish_cancelled_request(&mut self) -> Result<(), String> {
        let Some(deadline) = self.cancelled_reply_deadline.take() else {
            return Ok(());
        };
        // Motion STOP verification may consume the remaining deadline. A reply
        // already queued belongs to this one in-flight request and can safely
        // be retired; only waiting for a missing reply is deadline-limited.
        let response = match self.lines.try_recv() {
            Ok(response) => response,
            Err(mpsc::TryRecvError::Empty) => {
                self.receive(deadline.saturating_duration_since(Instant::now()))
            }
            Err(mpsc::TryRecvError::Disconnected) => Err(TransportError::HelperExited.into()),
        };
        let result = response
            .and_then(|line| parse_frame(&line).map(|(status, bytes)| (status.to_owned(), bytes)))
            .and_then(|(status, bytes)| match status.as_str() {
                "OK" => Ok(()),
                "ERR" => Err(format!(
                    "GPIB cancelled operation failed: {}",
                    String::from_utf8_lossy(&bytes)
                )),
                _ => Err(format!("Unknown GPIB helper status: {status}")),
            });
        if result.is_err() {
            self.invalidate();
        }
        result
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    fn request(&mut self, operation: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.request_cancellable(operation, payload, None)
    }
    fn request_cancellable(
        &mut self,
        operation: &str,
        payload: &[u8],
        cancel: Option<&AtomicBool>,
    ) -> Result<Vec<u8>, String> {
        if cancel.is_some_and(|token| token.load(Ordering::Acquire)) {
            return Err(TransportError::ReadCancelled.into());
        }
        if self.poisoned {
            return Err(TransportError::SessionFailed.into());
        }
        if self.cancelled_reply_deadline.is_some() {
            return Err(TransportError::ResponsePending.into());
        }
        if payload.len() > 4096 {
            return Err(TransportError::CommandTooLong.into());
        }
        let frame = format!(
            "{operation} {} 10000 {}\n",
            self.address,
            encode_hex(payload)
        );
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let result = (|| {
            self.input
                .write_all(frame.as_bytes())
                .map_err(|e| format!("Write GPIB helper: {e}"))?;
            self.input
                .flush()
                .map_err(|e| format!("Flush GPIB helper: {e}"))?;
            let line = self.receive_cancellable(COMMAND_TIMEOUT, cancel)?;
            parse_frame(&line).map(|(status, data)| (status.to_owned(), data))
        })();
        let (status, data) = match result {
            Ok(frame) => frame,
            Err(error) => {
                if error == TransportError::ReadCancelled.to_string() {
                    self.cancelled_reply_deadline = Some(deadline);
                    return Err(error);
                }
                self.poisoned = true;
                let _ = self.child.kill();
                return Err(error);
            }
        };
        if status == "ERR" {
            let message = String::from_utf8_lossy(&data);
            self.poisoned = true;
            let _ = self.child.kill();
            return Err(format!("GPIB helper: {message}"));
        }
        if status != "OK" {
            self.poisoned = true;
            let _ = self.child.kill();
            return Err(format!("Unknown GPIB helper status: {status}"));
        }
        Ok(data)
    }

    fn receive(&self, timeout: Duration) -> Result<String, String> {
        self.receive_cancellable(timeout, None)
    }
    fn receive_cancellable(
        &self,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.is_some_and(|token| token.load(Ordering::Acquire)) {
                return Err(TransportError::ReadCancelled.into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::ResponseTimedOut.into());
            }
            let slice = if cancel.is_some() {
                remaining.min(Duration::from_millis(20))
            } else {
                remaining
            };
            match self.lines.recv_timeout(slice) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(TransportError::HelperExited.into());
                }
            }
        }
    }
}

impl Drop for Gpib {
    fn drop(&mut self) {
        if self.poisoned {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return;
        }
        let _ = self
            .input
            .write_all(format!("EXIT {} 1000 -\n", self.address).as_bytes());
        let _ = self.input.flush();
        for _ in 0..20 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn find_helper() -> Result<PathBuf, String> {
    helper_paths()
        .0
        .ok_or_else(|| TransportError::NativeHelperNotFound.into())
}

/// Lists only VISA-reported GPIB resources.  This intentionally does not
/// probe addresses through the native adapter: Refresh must not reset or open
/// a shared GPIB bus.
pub fn available_gpib_resources() -> Result<Vec<String>, String> {
    let (native, visa) = helper_paths();
    if let Some(config) = hub_config()? {
        return hub_resources(&config, native.as_deref());
    }
    let visa = visa.ok_or_else(|| String::from(TransportError::VisaHelperNotFound))?;
    let mut resources = visa_resources(&visa)?
        .into_iter()
        .filter(|resource| is_gpib_resource(resource))
        .collect::<Vec<_>>();
    resources.sort();
    resources.dedup();
    Ok(resources)
}

/// An explicit hub setting is authoritative. Falling back to a direct helper
/// could reset a controller that a different application is already using.
fn hub_config() -> Result<Option<PathBuf>, String> {
    let Some(value) = std::env::var_os("KOHDALAB_INSTRUMENT_HUB_CONFIG") else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err("KOHDALAB_INSTRUMENT_HUB_CONFIG is set but empty.".into());
    }
    Ok(Some(PathBuf::from(value)))
}

fn hub_client(native: Option<&Path>) -> Result<PathBuf, String> {
    let explicit = std::env::var_os("KOHDALAB_INSTRUMENT_HUB_CLIENT").map(PathBuf::from);
    let candidate = explicit.or_else(|| {
        native
            .and_then(Path::parent)
            .map(|directory| directory.join(helper_name("kohdalab-instrument-hub")))
    });
    let Some(candidate) = candidate else {
        return Err("Instrument hub is enabled but its client executable was not found. Set KOHDALAB_INSTRUMENT_HUB_CLIENT or install it beside kohdalab-gpib-helper.".into());
    };
    if !candidate.is_file() {
        return Err(format!(
            "Instrument hub is enabled but its client executable was not found: {}",
            candidate.display()
        ));
    }
    Ok(candidate)
}

fn hub_resources(config: &Path, native: Option<&Path>) -> Result<Vec<String>, String> {
    let hub = hub_client(native)?;
    let args = vec![
        "--list".to_owned(),
        "--config".to_owned(),
        config.display().to_string(),
        "--app".to_owned(),
        "KohdaLab TRKR".to_owned(),
    ];
    let mut resources = resource_list(&hub, &args, "instrument hub")?
        .into_iter()
        .filter(|resource| is_gpib_resource(resource))
        .collect::<Vec<_>>();
    resources.sort();
    resources.dedup();
    Ok(resources)
}

fn helper_paths() -> (Option<PathBuf>, Option<PathBuf>) {
    let native_name = helper_name("kohdalab-gpib-helper");
    let visa_name = helper_name("kohdalab-visa-helper");
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("KOHDALAB_TRKR_HELPER_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("support"));
            candidates.push(parent.join("../Resources/support"));
        }
    }
    candidates.extend([
        PathBuf::from("portable/windows/native"),
        PathBuf::from("build/native-gpib"),
    ]);
    let native = candidates
        .iter()
        .map(|directory| directory.join(&native_name))
        .find(|path| path.is_file());
    let visa = candidates
        .iter()
        .map(|directory| directory.join(&visa_name))
        .find(|path| path.is_file());
    (native, visa)
}

fn helper_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn visa_resources(helper: &Path) -> Result<Vec<String>, String> {
    resource_list(helper, &["--list".to_owned()], "official VISA")
}

fn resource_list(helper: &Path, args: &[String], backend: &str) -> Result<Vec<String>, String> {
    let backend = backend.to_owned();
    let official_visa = backend == "official VISA";
    let mut command = Command::new(helper);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    suppress_console_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Start {backend} discovery: {e}"))?;
    let output = child.stdout.take().ok_or_else(|| {
        if official_visa {
            String::from(TransportError::VisaHelperStdoutUnavailable)
        } else {
            format!("{backend} discovery stdout was unavailable")
        }
    })?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader_backend = backend.clone();
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        let mut line = Vec::new();
        let reply = match reader
            .by_ref()
            .take((MAX_FRAME + 1) as u64)
            .read_until(b'\n', &mut line)
        {
            Ok(n) if n > 0 && n <= MAX_FRAME && line.last() == Some(&b'\n') => {
                Ok(String::from_utf8_lossy(&line[..n - 1]).into_owned())
            }
            Ok(_) if reader_backend == "official VISA" => {
                Err(TransportError::VisaDiscoveryNoBoundedFrame.into())
            }
            Ok(_) => Err(format!(
                "{reader_backend} discovery returned no bounded KIV frame"
            )),
            Err(error) if reader_backend == "official VISA" => {
                Err(format!("Read official VISA helper discovery: {error}"))
            }
            Err(error) => Err(format!("Read {reader_backend} discovery: {error}")),
        };
        let _ = sender.send(reply);
    });
    let reply = match receiver.recv_timeout(VISA_LIST_TIMEOUT) {
        Ok(reply) => reply,
        Err(RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if official_visa {
                TransportError::VisaDiscoveryTimedOut.into()
            } else {
                format!("{backend} discovery timed out")
            });
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if official_visa {
                TransportError::VisaDiscoveryExited.into()
            } else {
                format!("{backend} discovery exited unexpectedly")
            });
        }
    };
    // --list has no further useful output.  Killing after the first complete
    // frame also bounds a compromised helper that never exits.
    let _ = child.kill();
    let _ = child.wait();
    let reply = reply?;
    let (status, payload) = parse_frame(&reply)?;
    if status != "OK" {
        return Err(if official_visa {
            format!(
                "Official VISA helper discovery failed: {}",
                String::from_utf8_lossy(&payload)
            )
        } else {
            format!(
                "{backend} discovery failed: {}",
                String::from_utf8_lossy(&payload)
            )
        });
    }
    let listing = String::from_utf8(payload).map_err(|error| {
        if official_visa {
            format!("Official VISA resource list was not UTF-8: {error}")
        } else {
            format!("{backend} resource list was not UTF-8: {error}")
        }
    })?;
    let mut resources = Vec::new();
    for row in listing.lines() {
        let resource = row.split('\t').next().unwrap_or_default();
        if resource.is_empty() || !resource.is_ascii() || resource.len() > 255 {
            return Err(TransportError::InvalidVisaResourceList.into());
        }
        resources.push(resource.to_owned());
    }
    Ok(resources)
}

fn suppress_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW keeps the bundled console helpers invisible while
        // retaining their piped stdin/stdout protocol.
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn is_gpib_resource(resource: &str) -> bool {
    let mut fields = resource.split("::");
    let Some(interface) = fields.next() else {
        return false;
    };
    interface
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("GPIB"))
        && fields.next().is_some()
        && fields.next().is_some()
}

fn acquire_lease() -> Result<File, String> {
    #[cfg(not(test))]
    let root = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(not(test))]
    let directory = root.join(".kohdalab").join("iv-next-locks");
    #[cfg(test)]
    let directory =
        std::env::temp_dir().join(format!("kohdalab-trkr-lock-tests-{}", std::process::id()));
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("Create GPIB adapter lease directory: {e}"))?;
    let path = directory.join("gpib-adapter.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("Create GPIB adapter lease: {e}"))?;
    file.try_lock_exclusive()
        .map_err(|e| format!("GPIB adapter is already in use: {e}"))?;
    Ok(file)
}

fn validate_address(address: u8) -> Result<(), String> {
    if (1..=30).contains(&address) {
        Ok(())
    } else {
        Err(TransportError::InvalidAddress.into())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".to_owned()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value == "-" {
        return Ok(Vec::new());
    }
    if !value.is_ascii() {
        return Err(TransportError::InvalidHexPayload.into());
    }
    if value.len() % 2 != 0 || value.len() > MAX_PAYLOAD * 2 {
        return Err(TransportError::InvalidHexPayload.into());
    }
    (0..value.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&value[i..i + 2], 16)
                .map_err(|_| String::from(TransportError::InvalidHexPayload))
        })
        .collect()
}

fn parse_frame(frame: &str) -> Result<(&str, Vec<u8>), String> {
    let mut fields = frame.split('\t');
    if fields.next() != Some("KIV") {
        return Err(TransportError::MalformedHelperFrame.into());
    }
    let status = fields
        .next()
        .ok_or_else(|| String::from(TransportError::MalformedHelperStatus))?;
    let payload = fields
        .next()
        .ok_or_else(|| String::from(TransportError::MalformedHelperPayload))?;
    if fields.next().is_some() {
        return Err(TransportError::MalformedHelperFrame.into());
    }
    Ok((status, decode_hex(payload)?))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    static TEST_LEASE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn executable(path: &Path, source: &str) {
        std::fs::write(path, source).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn stop_retires_only_the_interrupted_reply_and_keeps_the_session() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("delayed-helper.sh");
        let marker = dir.path().join("started");
        executable(
            &helper,
            &format!(
                "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nIFS= read -r line\nprintf started > '{}'\nsleep 0.2\nprintf 'KIV\\tOK\\t6f6c64\\n'\nIFS= read -r line\nprintf 'KIV\\tOK\\t6672657368\\n'\nwhile IFS= read -r line; do :; done\n",
                marker.display()
            ),
        );
        let mut gpib = Gpib::open(&helper, 12).unwrap();
        let token = std::sync::Arc::new(AtomicBool::new(false));
        let cancel = token.clone();
        let stopper = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !marker.exists() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            }
            cancel.store(true, Ordering::Release);
        });
        let error = gpib.query_cancellable("first", &token).unwrap_err();
        stopper.join().unwrap();
        assert!(error.contains("cancelled"));
        assert!(!gpib.is_poisoned());
        assert!(
            gpib.query("must not be sent")
                .unwrap_err()
                .contains("pending")
        );
        gpib.finish_cancelled_request().unwrap();
        assert!(!gpib.is_poisoned());
        token.store(false, Ordering::Release);
        assert_eq!(gpib.query_cancellable("second", &token).unwrap(), "fresh");
    }

    #[test]
    fn already_received_cancelled_reply_can_be_retired_after_motion_stop_deadline() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("idle-helper.sh");
        executable(
            &helper,
            "#!/bin/sh\nprintf 'KIV\tOK\t-\n'\nwhile IFS= read -r line; do :; done\n",
        );
        let mut gpib = Gpib::open(&helper, 12).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(Ok("KIV\tOK\t6f6c64".into())).unwrap();
        gpib.lines = receiver;
        gpib.cancelled_reply_deadline = Some(Instant::now() - Duration::from_secs(1));
        gpib.finish_cancelled_request().unwrap();
        assert!(!gpib.is_poisoned());
        assert!(gpib.cancelled_reply_deadline.is_none());
    }

    #[test]
    fn failed_cancelled_reply_is_not_a_successful_recovery() {
        let _guard = TEST_LEASE.lock().unwrap();
        for response in ["KIV\\tERR\\t626164", "malformed frame"] {
            let dir = tempfile::tempdir().unwrap();
            let helper = dir.path().join("bad-helper.sh");
            executable(
                &helper,
                &format!(
                    "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nIFS= read -r line\nsleep 0.1\nprintf '{response}\\n'\nwhile IFS= read -r line; do :; done\n"
                ),
            );
            let mut gpib = Gpib::open(&helper, 12).unwrap();
            // Model a sent request cancelled before its reply arrives.
            gpib.input.write_all(b"QUERY 12 10000 58\n").unwrap();
            gpib.input.flush().unwrap();
            gpib.cancelled_reply_deadline = Some(Instant::now() + Duration::from_secs(2));
            assert!(gpib.finish_cancelled_request().is_err());
            assert!(gpib.is_poisoned());
        }
    }

    #[test]
    fn stalled_query_is_cancelled_promptly_and_late_replies_cannot_be_reused() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("stalled-helper.sh");
        let marker = dir.path().join("query-started");
        executable(
            &helper,
            &format!(
                "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nIFS= read -r line\nprintf started > '{}'\nwhile IFS= read -r line; do :; done\n",
                marker.display()
            ),
        );
        let mut gpib = Gpib::open(&helper, 12).unwrap();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let stopped_at = std::sync::Arc::new(std::sync::Mutex::new(None));
        let token = cancel.clone();
        let stopped = stopped_at.clone();
        let stopper = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !marker.exists() {
                assert!(Instant::now() < deadline, "query never started");
                thread::sleep(Duration::from_millis(5));
            }
            *stopped.lock().unwrap() = Some(Instant::now());
            token.store(true, Ordering::Release);
        });
        let error = gpib.query_cancellable("XY.\r", &cancel).unwrap_err();
        stopper.join().unwrap();
        assert!(error.contains("cancelled"));
        assert!(stopped_at.lock().unwrap().unwrap().elapsed() < Duration::from_secs(1));
        assert!(!gpib.is_poisoned());
        assert!(gpib.query("XY.\r").unwrap_err().contains("pending"));
        gpib.cancelled_reply_deadline = Some(Instant::now());
        assert!(gpib.finish_cancelled_request().is_err());
        assert!(gpib.is_poisoned());
        assert!(gpib.query("XY.\r").unwrap_err().contains("reconnect"));
    }

    #[test]
    fn cancellation_before_io_preserves_a_healthy_session() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("helper.sh");
        executable(
            &helper,
            "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nwhile IFS= read -r line; do printf 'KIV\\tOK\\t-\\n'; done\n",
        );
        let mut gpib = Gpib::open(&helper, 12).unwrap();
        let cancel = AtomicBool::new(true);
        assert!(
            gpib.query_cancellable("XY.\r", &cancel)
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(
            gpib.write_cancellable("DDEF 1,0\n", &cancel)
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(!gpib.is_poisoned());
        cancel.store(false, Ordering::Release);
        gpib.write_cancellable("DDEF 1,0\n", &cancel).unwrap();
    }

    #[test]
    fn fake_helper_round_trip_and_exit() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("helper.sh");
        executable(
            &helper,
            "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nwhile IFS= read -r line; do case \"$line\" in QUERY*) printf 'KIV\\tOK\\t52454144593F\\n' ;; EXIT*) printf 'KIV\\tOK\\t-\\n'; exit 0 ;; *) printf 'KIV\\tOK\\t-\\n' ;; esac; done\n",
        );
        let mut gpib = Gpib::open(&helper, 7).unwrap();
        assert_eq!(gpib.query("READ?").unwrap(), "READY?");
        gpib.write("CONF").unwrap();
    }

    #[test]
    fn rejects_invalid_address_and_frames() {
        assert!(validate_address(0).is_err());
        assert_eq!(encode_hex(&[]), "-");
        assert_eq!(encode_hex(b"*IDN?\n"), "2a49444e3f0a");
        assert!(parse_frame("KIV\tOK\tGG").is_err());
        assert!(parse_frame("KIV\tOK\tＡＡ").is_err());
        assert!(parse_frame("NOPE\tOK\t-").is_err());
    }

    #[test]
    fn fixed_transport_error_messages_preserve_the_public_contract() {
        for (error, message) in [
            (TransportError::ReadCancelled, "GPIB read cancelled"),
            (
                TransportError::ResponsePending,
                "GPIB cancelled reply pending; finish cancellation before further I/O",
            ),
            (
                TransportError::SessionFailed,
                "GPIB session has failed; reconnect before further I/O",
            ),
            (
                TransportError::CommandTooLong,
                "GPIB command exceeds 4096-byte limit",
            ),
            (
                TransportError::ResponseTimedOut,
                "GPIB helper response timed out",
            ),
            (
                TransportError::HelperExited,
                "GPIB helper exited unexpectedly",
            ),
            (
                TransportError::HelperFrameExceededSizeLimit,
                "GPIB helper frame exceeded size limit",
            ),
            (
                TransportError::InvalidHexPayload,
                "Invalid GPIB helper hex payload",
            ),
            (
                TransportError::MalformedHelperFrame,
                "Malformed GPIB helper frame",
            ),
            (
                TransportError::MalformedHelperStatus,
                "Malformed GPIB helper status",
            ),
            (
                TransportError::MalformedHelperPayload,
                "Malformed GPIB helper payload",
            ),
        ] {
            assert_eq!(String::from(error), message);
        }
    }

    #[test]
    fn malformed_reply_poisons_helper() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("malformed.sh");
        executable(
            &helper,
            "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nIFS= read -r line\nprintf 'garbage\\n'\nsleep 5\n",
        );
        let mut gpib = Gpib::open(&helper, 7).unwrap();
        assert_eq!(
            gpib.query("READ?").unwrap_err(),
            "Malformed GPIB helper frame"
        );
        assert!(gpib.poisoned);
        assert_eq!(
            gpib.query("READ?").unwrap_err(),
            "GPIB session has failed; reconnect before further I/O"
        );
    }

    #[test]
    fn requests_use_strict_lowercase_helper_hex_including_exit() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("strict-hex.sh");
        let payloads = dir.path().join("payloads.txt");
        executable(
            &helper,
            &format!(
                "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nwhile IFS= read -r line; do set -- $line; payload=$4; case \"$payload\" in *[!0123456789abcdef-]*) printf 'KIV\\tERR\\t62616420686578\\n'; exit 1 ;; esac; printf '%s\\n' \"$payload\" >> '{}'; case \"$1\" in QUERY) printf 'KIV\\tOK\\t6f6b\\n' ;; EXIT) printf 'KIV\\tOK\\t-\\n'; exit 0 ;; *) printf 'KIV\\tOK\\t-\\n' ;; esac; done\n",
                payloads.display()
            ),
        );
        {
            let mut gpib = Gpib::open(&helper, 12).unwrap();
            assert_eq!(gpib.query("*IDN?\n").unwrap(), "ok");
            gpib.write("XY.\r").unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(payloads).unwrap(),
            "2a49444e3f0a\n58592e0d\n-\n"
        );
    }

    #[test]
    fn connect_selects_exact_visa_resource_and_uses_wire_address_zero() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let visa = dir.path().join("visa.sh");
        let input = dir.path().join("visa-input.txt");
        let argument = dir.path().join("visa-argument.txt");
        let source = format!(
            "#!/bin/sh\nif [ \"$1\" = --list ]; then printf 'KIV\\tOK\\t47504942303A3A31323A3A494E535452094F6666696369616C20564953410A\\n'; exit 0; fi\nprintf '%s' \"$1\" > '{}'\nprintf 'KIV\\tOK\\t-\\n'\nIFS= read -r line\nprintf '%s' \"$line\" > '{}'\nprintf 'KIV\\tOK\\t4F4B\\n'\n",
            argument.display(),
            input.display()
        );
        executable(&visa, &source);
        let mut gpib = Gpib::connect_with_paths(12, None, Some(visa)).unwrap();
        assert_eq!(gpib.backend_name(), "official VISA");
        assert_eq!(gpib.query("Q?").unwrap(), "OK");
        assert_eq!(
            std::fs::read_to_string(argument).unwrap(),
            "GPIB0::12::INSTR"
        );
        assert!(
            std::fs::read_to_string(input)
                .unwrap()
                .starts_with("QUERY 0 10000")
        );
    }

    #[test]
    fn connect_keeps_native_address_when_visa_lists_no_gpib() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let visa = dir.path().join("visa.sh");
        let native = dir.path().join("native.sh");
        let input = dir.path().join("native-input.txt");
        executable(
            &visa,
            "#!/bin/sh\nprintf 'KIV\\tOK\\t555342303A3A3078313233343A3A3078353637383A3A534E3A3A494E535452094F6666696369616C20564953410A\\n'",
        );
        executable(
            &native,
            &format!(
                "#!/bin/sh\nprintf 'KIV\\tOK\\t-\\n'\nIFS= read -r line\nprintf '%s' \"$line\" > '{}'\nprintf 'KIV\\tOK\\t4F4B\\n'\n",
                input.display()
            ),
        );
        let mut gpib = Gpib::connect_with_paths(12, Some(native), Some(visa)).unwrap();
        assert_eq!(gpib.backend_name(), "native");
        assert_eq!(gpib.query("Q?").unwrap(), "OK");
        assert!(
            std::fs::read_to_string(input)
                .unwrap()
                .starts_with("QUERY 12 10000")
        );
    }

    #[test]
    fn another_visa_gpib_never_retries_through_native() {
        let _guard = TEST_LEASE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let visa = dir.path().join("visa.sh");
        let native = dir.path().join("native.sh");
        let marker = dir.path().join("native-started.txt");
        executable(
            &visa,
            "#!/bin/sh\nprintf 'KIV\\tOK\\t47504942303A3A373A3A494E535452094F6666696369616C20564953410A\\n'",
        );
        executable(
            &native,
            &format!(
                "#!/bin/sh\nprintf started > '{}'\nprintf 'KIV\\tOK\\t-\\n'\n",
                marker.display()
            ),
        );
        let error = match Gpib::connect_with_paths(12, Some(native), Some(visa)) {
            Ok(_) => panic!("a mismatched VISA GPIB resource must not use native fallback"),
            Err(error) => error,
        };
        assert!(error.contains("GPIB0::7::INSTR"));
        assert!(!marker.exists());
    }
    #[test]
    fn opt_in_hub_is_authoritative_and_preserves_cancelled_reply_boundaries() {
        const CHILD_CASE: &str = "KOHDALAB_TRKR_HUB_TEST_CASE";
        if let Ok(case) = std::env::var(CHILD_CASE) {
            let helper = PathBuf::from(std::env::var_os("KOHDALAB_TRKR_HUB_TEST_DIRECT").unwrap());
            let marker = PathBuf::from(std::env::var_os("KOHDALAB_TRKR_HUB_TEST_MARKER").unwrap());
            if case != "healthy" {
                assert!(Gpib::open(&helper, 12).is_err());
                assert!(Gpib::connect_with_paths(12, Some(helper.clone()), Some(helper)).is_err());
                assert!(available_gpib_resources().is_err());
                assert!(
                    !marker.exists(),
                    "a selected hub must never open native/VISA as fallback"
                );
                return;
            }
            for direct in [true, false] {
                let mut gpib = if direct {
                    Gpib::open(&helper, 12).unwrap()
                } else {
                    Gpib::connect_with_paths(12, Some(helper.clone()), Some(helper.clone()))
                        .unwrap()
                };
                assert_eq!(gpib.backend_name(), "instrument hub");
                assert_eq!(gpib.query("second").unwrap(), "fresh");
            }
            assert_eq!(
                available_gpib_resources().unwrap(),
                vec!["GPIB0::12::INSTR"]
            );
            let mut gpib = Gpib::open(&helper, 12).unwrap();
            let seen = PathBuf::from(std::env::var_os("KOHDALAB_TRKR_HUB_TEST_QUERY").unwrap());
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let token = cancel.clone();
            let stopper = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(3);
                while !seen.exists() {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(2));
                }
                token.store(true, Ordering::Release);
            });
            assert!(
                gpib.query_cancellable("first", &cancel)
                    .unwrap_err()
                    .contains("cancelled")
            );
            stopper.join().unwrap();
            assert!(gpib.query("not sent").unwrap_err().contains("pending"));
            gpib.finish_cancelled_request().unwrap();
            cancel.store(false, Ordering::Release);
            assert_eq!(gpib.query_cancellable("second", &cancel).unwrap(), "fresh");
            assert!(!gpib.is_poisoned());
            assert!(!marker.exists());
            let args =
                std::fs::read_to_string(std::env::var_os("KOHDALAB_TRKR_HUB_TEST_ARGS").unwrap())
                    .unwrap();
            assert!(args.lines().any(|a| a == "KohdaLab TRKR"));
            assert!(args.lines().any(|a| a == "12"));
            assert!(
                args.lines()
                    .any(|a| a.ends_with("endpoint with spaces.json"))
            );
            return;
        }
        // Every environment choice is confined to a fresh test process; never
        // mutate process-global environment while other Rust tests run.
        for case in ["healthy", "empty_config", "missing_client", "denied_client"] {
            let dir = tempfile::tempdir().unwrap();
            let helper = dir.path().join("direct.sh");
            let hub = dir.path().join("hub.sh");
            let endpoint = dir.path().join("endpoint with spaces.json");
            std::fs::write(&endpoint, "{}").unwrap();
            executable(
                &helper,
                "#!/bin/sh\n: > \"$KOHDALAB_TRKR_HUB_TEST_MARKER\"\nprintf 'KIV\\tOK\\t-\\n'\n",
            );
            let listing = encode_hex(b"GPIB0::12::INSTR\nUSB0::1::INSTR\nGPIB0::12::INSTR\n");
            executable(
                &hub,
                &format!(
                    r#"#!/bin/sh
printf '%s\n' "$@" > "$KOHDALAB_TRKR_HUB_TEST_ARGS"
if [ "$KOHDALAB_TRKR_HUB_TEST_CASE" = denied_client ]; then
  printf 'KIV\tERR\t64656e696564\n'; exit 1
fi
if [ "$1" = --list ]; then
  printf 'KIV\tOK\t{listing}\n'; exit 0
fi
printf 'KIV\tOK\t-\n'
while IFS= read -r line; do
  case "$line" in
    *6669727374) : > "$KOHDALAB_TRKR_HUB_TEST_QUERY"; sleep 0.2; printf 'KIV\tOK\t6f6c64\n' ;;
    QUERY*) printf 'KIV\tOK\t6672657368\n' ;;
    EXIT*) exit 0 ;;
  esac
done
"#
                ),
            );
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::opt_in_hub_is_authoritative_and_preserves_cancelled_reply_boundaries",
                    "--nocapture",
                ])
                .env(CHILD_CASE, case)
                .env("KOHDALAB_TRKR_HUB_TEST_DIRECT", &helper)
                .env(
                    "KOHDALAB_TRKR_HUB_TEST_MARKER",
                    dir.path().join("direct-called"),
                )
                .env(
                    "KOHDALAB_TRKR_HUB_TEST_QUERY",
                    dir.path().join("query-seen"),
                )
                .env("KOHDALAB_TRKR_HUB_TEST_ARGS", dir.path().join("args"))
                .env(
                    "KOHDALAB_INSTRUMENT_HUB_CONFIG",
                    if case == "empty_config" {
                        std::ffi::OsStr::new("")
                    } else {
                        endpoint.as_os_str()
                    },
                )
                .env(
                    "KOHDALAB_INSTRUMENT_HUB_CLIENT",
                    if case == "missing_client" {
                        dir.path().join("missing-client")
                    } else {
                        hub
                    },
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
