//! Connection leases shared by views on one worker, never by competing drivers.
//!
//! Views returned by [`MotionPortPool`] share one serial session per port. A
//! view may additionally claim its session for a single executor. The claim is
//! enforced on every exchange, so no other view can interleave commands with
//! the owner (including STOP) until every view holding the claim is dropped.
use crate::{Port, protocol::Bus};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering as ClaimOrdering},
    },
};

struct Inner {
    bus: Box<dyn Bus + Send>,
    /// Zero when unclaimed; otherwise the token of the owning executor.
    owner: u64,
}
type Connection = Mutex<Inner>;

static NEXT_CLAIM: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    family: String,
    baud: u32,
    timeout: f64,
    write_end: String,
    read_end: String,
}

#[derive(Default)]
pub struct MotionPortPool {
    ports: HashMap<String, (Settings, Weak<Connection>)>,
}

impl MotionPortPool {
    pub(crate) fn open(
        &mut self,
        name: &str,
        family: &str,
        baud: u32,
        timeout: f64,
        write_end: &str,
        read_end: &str,
    ) -> Result<Box<dyn Bus + Send>, String> {
        let key = name.trim().to_ascii_uppercase();
        let settings = Settings {
            family: family.into(),
            baud,
            timeout,
            write_end: write_end.into(),
            read_end: read_end.into(),
        };
        self.lease(key, settings, || {
            Ok(Box::new(Port::open(
                name, baud, timeout, write_end, read_end,
            )?))
        })
    }

    fn lease(
        &mut self,
        key: String,
        settings: Settings,
        open: impl FnOnce() -> Result<Box<dyn Bus + Send>, String>,
    ) -> Result<Box<dyn Bus + Send>, String> {
        if let Some((previous, weak)) = self.ports.get(&key)
            && let Some(connection) = weak.upgrade()
        {
            if *previous != settings {
                return Err(format!("Conflicting controller settings for {key}"));
            }
            return Ok(Box::new(SharedBus::new(connection)));
        }
        let connection = shared_connection(open()?);
        self.ports
            .insert(key, (settings, Arc::downgrade(&connection)));
        Ok(Box::new(SharedBus::new(connection)))
    }
}

fn shared_connection(bus: Box<dyn Bus + Send>) -> Arc<Connection> {
    Arc::new(Mutex::new(Inner { bus, owner: 0 }))
}

/// Wrap an embedder-supplied transport with the same sharing and exclusive
/// ownership rules as pooled serial ports, for use with
/// [`crate::MotionRig::open_with_bus_factory`] and simulations.
pub fn shareable_bus(bus: Box<dyn Bus + Send>) -> Box<dyn Bus + Send> {
    Box::new(SharedBus::new(shared_connection(bus)))
}

/// Releases a claim when the last view holding it is dropped.
struct Claim {
    connection: Arc<Connection>,
    token: u64,
}
impl Drop for Claim {
    fn drop(&mut self) {
        let mut inner = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.owner == self.token {
            inner.owner = 0;
        }
    }
}

struct SharedBus {
    connection: Arc<Connection>,
    claim: Option<Arc<Claim>>,
}
impl SharedBus {
    fn new(connection: Arc<Connection>) -> Self {
        Self {
            connection,
            claim: None,
        }
    }
    fn locked(&self) -> Result<MutexGuard<'_, Inner>, String> {
        let inner = self
            .connection
            .lock()
            .map_err(|_| "Motion connection worker failed".to_owned())?;
        let actor = self.claim.as_ref().map_or(0, |claim| claim.token);
        if inner.owner != 0 && inner.owner != actor {
            return Err("Motion connection is exclusively owned by another motion executor".into());
        }
        Ok(inner)
    }
}
impl Bus for SharedBus {
    fn shared_clone(&self) -> Option<Box<dyn Bus + Send>> {
        Some(Box::new(Self {
            connection: self.connection.clone(),
            claim: self.claim.clone(),
        }))
    }
    fn connection_id(&self) -> Option<usize> {
        Some(Arc::as_ptr(&self.connection) as *const () as usize)
    }
    fn exclusive_clone(&self) -> Result<Box<dyn Bus + Send>, String> {
        let mut inner = self
            .connection
            .lock()
            .map_err(|_| "Motion connection worker failed".to_owned())?;
        if inner.owner != 0 {
            return Err("Motion connection is already exclusively owned".into());
        }
        let token = NEXT_CLAIM.fetch_add(1, ClaimOrdering::Relaxed);
        inner.owner = token;
        drop(inner);
        Ok(Box::new(Self {
            connection: self.connection.clone(),
            claim: Some(Arc::new(Claim {
                connection: self.connection.clone(),
                token,
            })),
        }))
    }
    fn begin_recording(&mut self) {
        if let Ok(mut inner) = self.locked() {
            inner.bus.begin_recording();
        }
    }
    fn take_recording(&mut self) -> Vec<crate::MotionExchange> {
        self.locked()
            .map(|mut inner| inner.bus.take_recording())
            .unwrap_or_default()
    }
    fn query(&mut self, command: &str) -> Result<String, String> {
        self.locked()?.bus.query(command)
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        self.locked()?.bus.write(command)
    }
    fn emergency_write(&mut self, command: &str) -> Result<(), String> {
        self.locked()?.bus.emergency_write(command)
    }
    fn stop_query(&mut self, command: &str) -> Result<String, String> {
        self.locked()?.bus.stop_query(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Delay, DelayConfig, MotionRig, Scanner, ScannerConfig};
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct FakeBus {
        log: Arc<Mutex<Vec<String>>>,
        replies: VecDeque<Result<String, String>>,
    }
    impl Bus for FakeBus {
        fn query(&mut self, command: &str) -> Result<String, String> {
            self.log.lock().unwrap().push(command.into());
            self.replies
                .pop_front()
                .unwrap_or_else(|| Err(format!("unexpected query {command}")))
        }
        fn write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            Ok(())
        }
        fn emergency_write(&mut self, command: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(command.into());
            Ok(())
        }
    }

    fn shared_fake(
        log: Arc<Mutex<Vec<String>>>,
        replies: &[&str],
    ) -> (Arc<Connection>, Box<dyn Bus + Send>) {
        let connection = shared_connection(Box::new(FakeBus {
            log,
            replies: replies.iter().map(|item| Ok((*item).into())).collect(),
        }));
        (connection.clone(), Box::new(SharedBus::new(connection)))
    }

    #[test]
    fn selected_view_shares_existing_leases_without_io_and_routes_to_source_ports() {
        let delay_log = Arc::new(Mutex::new(Vec::new()));
        let scanner_log = Arc::new(Mutex::new(Vec::new()));
        let (delay_connection, delay_port) = shared_fake(delay_log.clone(), &["0", "OK", "R"]);
        let (scanner_connection, scanner_port) =
            shared_fake(scanner_log.clone(), &["1TP1.0000", "1TS000032"]);
        let delay_config = DelayConfig {
            port: "delay".into(),
            stage: Some("SGSP46-500".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let scanner_config = ScannerConfig {
            port: "scanner".into(),
            actuator: Some("TRA12CC".into()),
            sample_um_per_unit: 1.0,
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let delay_rig = MotionRig {
            ports: vec![delay_port],
            delay: Some(Delay {
                config: delay_config,
                port: 0,
                micro: 1,
            }),
            scanners: [None, None],
        };
        let scanner_rig = MotionRig {
            ports: vec![scanner_port],
            delay: None,
            scanners: [
                Some(Scanner {
                    config: scanner_config,
                    port: 0,
                    prepared: false,
                }),
                None,
            ],
        };
        let delay_refs_before = Arc::strong_count(&delay_connection);
        let scanner_refs_before = Arc::strong_count(&scanner_connection);
        let mut selected = MotionRig::selected_view(Some(&delay_rig), Some(&scanner_rig)).unwrap();

        assert!(delay_log.lock().unwrap().is_empty());
        assert!(scanner_log.lock().unwrap().is_empty());
        assert_eq!(Arc::strong_count(&delay_connection), delay_refs_before + 1);
        assert_eq!(
            Arc::strong_count(&scanner_connection),
            scanner_refs_before + 1
        );

        selected.position().unwrap();
        selected.stop().unwrap();
        assert_eq!(*delay_log.lock().unwrap(), ["Q:", "L:1", "!:"]);
        assert_eq!(*scanner_log.lock().unwrap(), ["1TP", "1ST", "1TS"]);
    }

    #[test]
    fn pool_reuses_one_transport_and_rejects_different_settings() {
        let mut pool = MotionPortPool::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let opened = Arc::new(AtomicUsize::new(0));
        let settings = Settings {
            family: "CONEXCC".into(),
            baud: 921_600,
            timeout: 1.0,
            write_end: "\r\n".into(),
            read_end: "\r\n".into(),
        };
        let first_log = log.clone();
        let first_opened = opened.clone();
        let mut first = pool
            .lease("COM4".into(), settings.clone(), move || {
                first_opened.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(FakeBus {
                    log: first_log,
                    replies: [Ok("first".into()), Ok("second".into())].into(),
                }))
            })
            .unwrap();
        let second_opened = opened.clone();
        let mut second = pool
            .lease("COM4".into(), settings.clone(), move || {
                second_opened.fetch_add(1, Ordering::SeqCst);
                Err("must not open a second transport".into())
            })
            .unwrap();
        assert_eq!(opened.load(Ordering::SeqCst), 1);
        assert_eq!(first.query("first").unwrap(), "first");
        assert_eq!(second.query("second").unwrap(), "second");
        assert_eq!(*log.lock().unwrap(), ["first", "second"]);
        let mut incompatible = settings;
        incompatible.baud = 9_600;
        assert!(
            pool.lease("COM4".into(), incompatible, || Err("must not run".into()))
                .is_err()
        );
    }

    #[test]
    fn exclusive_claim_refuses_every_other_view_until_released() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (_connection, mut shared) = shared_fake(log.clone(), &["owner", "after"]);
        let mut other = shared.shared_clone().unwrap();
        assert_eq!(shared.connection_id(), other.connection_id());
        let mut owner = shared.exclusive_clone().unwrap();
        // Claims never nest, even from the owner itself.
        assert!(shared.exclusive_clone().is_err());
        assert!(owner.exclusive_clone().is_err());
        // Ordinary views cannot query, write or even STOP the owned session.
        assert!(shared.query("denied").is_err());
        assert!(other.write("denied").is_err());
        assert!(other.emergency_write("denied").is_err());
        assert!(other.stop_query("denied").is_err());
        assert!(log.lock().unwrap().is_empty());
        // Views derived from the owner keep its claim.
        let mut derived = owner.shared_clone().unwrap();
        assert_eq!(owner.query("owner").unwrap(), "owner");
        drop(owner);
        assert!(other.query("still owned").is_err());
        derived.write("derived").unwrap();
        drop(derived);
        assert_eq!(other.query("after").unwrap(), "after");
        assert_eq!(*log.lock().unwrap(), ["owner", "derived", "after"]);
        // Released connections can be claimed again.
        drop(shared.exclusive_clone().unwrap());
    }

    fn cc(port: &str, address: u8) -> crate::ScannerConfig {
        ScannerConfig {
            port: port.into(),
            actuator: Some("TRA12CC".into()),
            controller_address: address,
            sample_um_per_unit: 1.0,
            ..Default::default()
        }
        .normalized()
        .unwrap()
    }

    fn scanner_rig(ports: Vec<Box<dyn Bus + Send>>, x: (usize, u8), y: (usize, u8)) -> MotionRig {
        MotionRig {
            ports,
            delay: None,
            scanners: [
                Some(Scanner {
                    config: cc(&format!("COM{}", x.0 + 4), x.1),
                    port: x.0,
                    prepared: false,
                }),
                Some(Scanner {
                    config: cc(&format!("COM{}", y.0 + 4), y.1),
                    port: y.0,
                    prepared: false,
                }),
            ],
        }
    }

    #[test]
    fn split_moves_only_requested_axes_into_exclusive_ownership() {
        let x_log = Arc::new(Mutex::new(Vec::new()));
        let y_log = Arc::new(Mutex::new(Vec::new()));
        let (_x, x_port) = shared_fake(x_log.clone(), &["1TP6", "1TP6"]);
        let (_y, y_port) = shared_fake(y_log.clone(), &["1TP6", "1TP6"]);
        let rig = scanner_rig(vec![x_port, y_port], (0, 1), (1, 1));
        assert!(rig.split_exclusive([false; 3]).is_err());
        assert!(rig.split_exclusive([true, false, false]).is_err());
        let (mut remaining, mut owned) = rig.split_exclusive([false, true, false]).unwrap();
        assert_eq!(remaining.connected_axes(), [false, false, true]);
        assert_eq!(owned.connected_axes(), [false, true, false]);
        // A second split cannot claim X again while it is owned.
        assert!(rig.split_exclusive([false, true, false]).is_err());
        assert_eq!(owned.position().unwrap().physical[1], Some(0.0));
        assert_eq!(remaining.position().unwrap().physical[2], Some(0.0));
        assert_eq!(*x_log.lock().unwrap(), ["1TP"]);
        drop(owned);
        let (_remaining, owned) = rig.split_exclusive([false, true, false]).unwrap();
        drop(owned);
    }

    #[test]
    fn split_refuses_an_axis_sharing_a_connection_or_port_with_a_kept_axis() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (_shared, port) = shared_fake(log.clone(), &[]);
        // X and Y on one serial session (addresses 1 and 2).
        let rig = scanner_rig(vec![port], (0, 1), (0, 2));
        for axes in [[false, true, false], [false, false, true]] {
            assert!(rig.split_exclusive(axes).is_err(), "{axes:?}");
        }
        // Taking both is one owner of one connection.
        let (remaining, owned) = rig.split_exclusive([false, true, true]).unwrap();
        assert_eq!(remaining.connected_axes(), [false; 3]);
        assert_eq!(owned.ports.len(), 1);
        drop(owned);
        // Distinct sessions whose names alias one device are refused too.
        let (_a, a) = shared_fake(log.clone(), &[]);
        let (_b, b) = shared_fake(log.clone(), &[]);
        let mut rig = scanner_rig(vec![a, b], (0, 1), (1, 1));
        rig.scanners[1].as_mut().unwrap().config.port = "\\\\.\\com4".into();
        assert!(rig.split_exclusive([false, true, false]).is_err());
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn transports_without_a_shared_identity_cannot_be_split() {
        let rig = MotionRig {
            ports: vec![Box::new(FakeBus {
                log: Arc::new(Mutex::new(Vec::new())),
                replies: VecDeque::new(),
            })],
            delay: None,
            scanners: [
                Some(Scanner {
                    config: cc("COM4", 1),
                    port: 0,
                    prepared: false,
                }),
                None,
            ],
        };
        assert!(rig.split_exclusive([false, true, false]).is_err());
    }
}
