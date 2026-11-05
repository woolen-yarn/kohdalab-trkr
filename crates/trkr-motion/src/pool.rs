//! Connection leases shared by views on one worker, never by competing drivers.
use crate::{Port, protocol::Bus};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

type Connection = Mutex<Box<dyn Bus + Send>>;

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
            return Ok(Box::new(SharedBus(connection)));
        }
        let connection: Arc<Connection> = Arc::new(Mutex::new(open()?));
        self.ports
            .insert(key, (settings, Arc::downgrade(&connection)));
        Ok(Box::new(SharedBus(connection)))
    }
}

struct SharedBus(Arc<Connection>);
impl SharedBus {
    fn locked(&self) -> Result<std::sync::MutexGuard<'_, Box<dyn Bus + Send>>, String> {
        self.0
            .lock()
            .map_err(|_| "Motion connection worker failed".into())
    }
}
impl Bus for SharedBus {
    fn shared_clone(&self) -> Option<Box<dyn Bus + Send>> {
        Some(Box::new(Self(self.0.clone())))
    }
    fn begin_recording(&mut self) {
        if let Ok(mut bus) = self.locked() {
            bus.begin_recording();
        }
    }
    fn take_recording(&mut self) -> Vec<crate::MotionExchange> {
        self.locked()
            .map(|mut bus| bus.take_recording())
            .unwrap_or_default()
    }
    fn query(&mut self, command: &str) -> Result<String, String> {
        self.locked()?.query(command)
    }
    fn write(&mut self, command: &str) -> Result<(), String> {
        self.locked()?.write(command)
    }
    fn emergency_write(&mut self, command: &str) -> Result<(), String> {
        self.locked()?.emergency_write(command)
    }
    fn stop_query(&mut self, command: &str) -> Result<String, String> {
        self.locked()?.stop_query(command)
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
        let connection: Arc<Connection> = Arc::new(Mutex::new(Box::new(FakeBus {
            log,
            replies: replies.iter().map(|item| Ok((*item).into())).collect(),
        })));
        (connection.clone(), Box::new(SharedBus(connection)))
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
}
