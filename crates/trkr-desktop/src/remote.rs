//! Opt-in loopback bridge. Requests are executed on the GUI thread, never on a
//! second hardware controller. No instrument I/O occurs in the network thread.
use serde_json::{Value, json};
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_REQUEST: u64 = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);
pub struct Request {
    pub body: Value,
    pub deadline: Instant,
    pub reply: mpsc::SyncSender<Value>,
}
pub struct Server {
    pub requests: mpsc::Receiver<Request>,
    pub descriptor: PathBuf,
    stopped: Arc<AtomicBool>,
}
impl Server {
    pub fn start() -> Result<Self, String> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .ok_or("Cannot locate the user profile for Python API discovery")?;
        Self::in_directory(PathBuf::from(home).join(".kohdalab/run"))
    }
    pub(crate) fn in_directory(directory: PathBuf) -> Result<Self, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| format!("Python API: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = uuid::Uuid::new_v4().to_string();
        let instance = uuid::Uuid::new_v4().to_string();
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let descriptor = directory.join(format!("trkr-gui-{}-{instance}.json", std::process::id()));
        // NamedTempFile is private (0600 on Unix); publish only a complete file.
        let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        serde_json::to_writer(
            &mut file,
            &json!({"revision":1,"host":"127.0.0.1","port":port,
            "token":token,"instance_id":instance,"pid":std::process::id()}),
        )
        .map_err(|e| e.to_string())?;
        file.persist_noclobber(&descriptor)
            .map_err(|e| e.to_string())?;
        let (send, requests) = mpsc::sync_channel(8);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, peer)) if peer.ip().is_loopback() => {
                        let _ = serve(stream, &token, &send);
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            requests,
            descriptor,
            stopped,
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = std::fs::remove_file(&self.descriptor);
        // Never hold up Controller::drop and its hardware Stop while a peer is
        // idle. The detached listener exits after its bounded socket timeout;
        // dropping the request receiver prevents any late command dispatch.
    }
}
fn serve(
    mut stream: TcpStream,
    token: &str,
    send: &mpsc::SyncSender<Request>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let line = read_request(&mut stream)?;
    let result = if line.len() as u64 > MAX_REQUEST || !line.ends_with('\n') {
        Err("Request must be one JSON line smaller than 1 MiB".into())
    } else {
        serde_json::from_str::<Value>(&line)
            .map_err(|_| "Invalid JSON request".to_owned())
            .and_then(|body| {
                if body.get("token").and_then(Value::as_str) != Some(token) {
                    return Err("Python API authentication failed".into());
                }
                let (reply, receive) = mpsc::sync_channel(1);
                send.try_send(Request {
                    body,
                    deadline: Instant::now() + TIMEOUT,
                    reply,
                })
                .map_err(|_| "GUI request queue unavailable".to_owned())?;
                receive
                    .recv_timeout(TIMEOUT)
                    .map_err(|_| "GUI request timed out; check status before retrying".to_owned())
            })
    };
    let response = result.unwrap_or_else(|error| json!({"ok":false,"error":error}));
    serde_json::to_writer(&mut stream, &response).map_err(|e| e.to_string())?;
    stream.write_all(b"\n").map_err(|e| e.to_string())
}

fn read_request(stream: &mut TcpStream) -> Result<String, String> {
    use std::io::Read;
    let deadline = Instant::now() + TIMEOUT;
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Request read timed out")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Incomplete JSON request".into());
        }
        let end = chunk[..count]
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1);
        bytes.extend_from_slice(&chunk[..end.unwrap_or(count)]);
        if bytes.len() as u64 > MAX_REQUEST {
            return Err("Request exceeds 1 MiB".into());
        }
        if end.is_some() {
            return String::from_utf8(bytes).map_err(|_| "Request must be UTF-8".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    fn request(descriptor: &Value, token: &str) -> Value {
        let mut socket =
            TcpStream::connect(("127.0.0.1", descriptor["port"].as_u64().unwrap() as u16)).unwrap();
        writeln!(socket, "{}", json!({"token":token,"action":"status"})).unwrap();
        let mut response = String::new();
        BufReader::new(socket).read_line(&mut response).unwrap();
        serde_json::from_str(&response).unwrap()
    }
    #[test]
    fn authenticates_before_dispatch_and_cleans_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::in_directory(dir.path().into()).unwrap();
        let descriptor: Value =
            serde_json::from_slice(&std::fs::read(&server.descriptor).unwrap()).unwrap();
        assert_eq!(descriptor["host"], "127.0.0.1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&server.descriptor)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert_eq!(request(&descriptor, "invalid")["ok"], false);
        assert!(server.requests.try_recv().is_err());
        let copy = descriptor.clone();
        let client = thread::spawn(move || request(&copy, copy["token"].as_str().unwrap()));
        let job = server.requests.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(job.body["action"], "status");
        job.reply
            .send(json!({"ok":true,"result":{"running":false}}))
            .unwrap();
        assert_eq!(client.join().unwrap()["result"]["running"], false);
        let path = server.descriptor.clone();
        drop(server);
        assert!(!path.exists());
    }
}
