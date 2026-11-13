//! Opt-in loopback bridge. Requests are executed on the GUI thread, never on a
//! second hardware controller. No instrument I/O occurs in the network thread.
use crate::config::UserConfigPaths;
use serde_json::{Value, json};
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_REQUEST: u64 = 1024 * 1024;
const MAX_CONNECTIONS: usize = 8;
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
        Self::in_directory(UserConfigPaths::for_current_user().run_dir())
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
        let legacy = json!({"revision":1,"host":"127.0.0.1","port":port,
            "token":token,"instance_id":instance,"pid":std::process::id()});
        let descriptor_value = kohdalab_control_protocol::descriptor(legacy, "kohdalab-trkr", 1);
        serde_json::to_writer(&mut file, &descriptor_value).map_err(|e| e.to_string())?;
        file.persist_noclobber(&descriptor)
            .map_err(|e| e.to_string())?;
        let (send, requests) = mpsc::sync_channel(8);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let active = Arc::new(AtomicUsize::new(0));
        thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, peer)) if peer.ip().is_loopback() => {
                        // Bound socket work, while all GUI/device actions remain serialized.
                        if let Some(permit) = ConnectionPermit::acquire(&active) {
                            let token = token.clone();
                            let send = send.clone();
                            let _ = thread::Builder::new().name("trkr-api-peer".into()).spawn(
                                move || {
                                    let _permit = permit;
                                    let _ = serve(stream, &token, &send);
                                },
                            );
                        } // At capacity, close this socket immediately; never queue peers.
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
struct ConnectionPermit(Arc<AtomicUsize>);
impl ConnectionPermit {
    fn acquire(active: &Arc<AtomicUsize>) -> Option<Self> {
        let mut count = active.load(Ordering::Acquire);
        while count < MAX_CONNECTIONS {
            match active.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self(active.clone())),
                Err(current) => count = current,
            }
        }
        None
    }
}
impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
fn serve(
    mut stream: TcpStream,
    token: &str,
    send: &mpsc::SyncSender<Request>,
) -> Result<(), String> {
    // macOS may inherit O_NONBLOCK from the accepting listener. Workers use
    // bounded blocking reads, so normalize the accepted socket explicitly.
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let line = read_request(&mut stream)?;
    let mut common_original = None;
    let result = if line.len() as u64 > MAX_REQUEST || !line.ends_with('\n') {
        Err("Request must be one JSON line smaller than 1 MiB".into())
    } else {
        serde_json::from_str::<Value>(&line)
            .map_err(|_| "Invalid JSON request".to_owned())
            .and_then(|body| {
                if kohdalab_control_protocol::is_common(&body) {
                    common_original = Some(body.clone());
                }
                if body.get("token").and_then(Value::as_str) != Some(token) {
                    let native = json!({"ok":false,"error":"Python API authentication failed","error_code":"authentication"});
                    return Ok(if kohdalab_control_protocol::is_common(&body) {
                        kohdalab_control_protocol::response(&body, native)
                    } else {
                        native
                    });
                }
                let original = body.clone();
                let body = match kohdalab_control_protocol::prepare(&body) {
                    Ok(body) => body,
                    Err(error) => return Ok(error),
                };
                let (reply, receive) = mpsc::sync_channel(1);
                send.try_send(Request {
                    body,
                    deadline: Instant::now() + TIMEOUT,
                    reply,
                })
                .map_err(|_| "GUI request queue unavailable".to_owned())?;
                let native = receive.recv_timeout(TIMEOUT).map_err(|_| {
                    "GUI request timed out; check status before retrying".to_owned()
                })?;
                Ok(kohdalab_control_protocol::response(&original, native))
            })
    };
    let response = result.unwrap_or_else(|error| {
        let native = json!({"ok":false,"error":error,"error_code":"transport"});
        common_original.as_ref().map_or(native.clone(), |original| {
            kohdalab_control_protocol::response(original, native)
        })
    });
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
    fn idle_peer_does_not_block_authenticated_status() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::in_directory(dir.path().into()).unwrap();
        let descriptor: Value =
            serde_json::from_slice(&std::fs::read(&server.descriptor).unwrap()).unwrap();
        let _idle =
            TcpStream::connect(("127.0.0.1", descriptor["port"].as_u64().unwrap() as u16)).unwrap();
        let copy = descriptor.clone();
        let client = thread::spawn(move || request(&copy, copy["token"].as_str().unwrap()));
        // Read timeout on the idle peer is 2 s; this request must bypass it.
        let job = server
            .requests
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        job.reply.send(json!({"ok":true,"result":{}})).unwrap();
        assert_eq!(client.join().unwrap()["ok"], true);
    }
    #[test]
    fn connection_admission_is_bounded_and_permits_are_reusable() {
        let active = Arc::new(AtomicUsize::new(0));
        let permits: Vec<_> = (0..MAX_CONNECTIONS)
            .map(|_| ConnectionPermit::acquire(&active).unwrap())
            .collect();
        assert!(ConnectionPermit::acquire(&active).is_none());
        assert_eq!(active.load(Ordering::Acquire), MAX_CONNECTIONS);
        drop(permits);
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert!(ConnectionPermit::acquire(&active).is_some());
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
