//! Translate native driver measurements into the same scientific record used
//! by the VISA backend. Native API calls remain native in the raw event log.
use super::*;
use trkr_transport::thorlabs::{
    DriverExchange, MeterIdentity, MeterSnapshot, PowerSample, PowerUnit, ThorlabsMeter,
};

// measPower starts a fresh integration and may legitimately take seconds.
// Leave time for surrounding metadata reads and one queued live integration
// within the native controller's 25 s request budget.
const DRIVER_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) trait Driver: Send {
    fn identity(&self) -> &MeterIdentity;
    fn set_wavelength_nm(&mut self, nm: f64) -> Result<f64, String>;
    fn snapshot(&mut self) -> Result<MeterSnapshot, String>;
    fn read_power(&mut self) -> Result<PowerSample, String>;
    fn take_exchanges(&mut self) -> Vec<DriverExchange>;
}

impl Driver for ThorlabsMeter {
    fn identity(&self) -> &MeterIdentity {
        self.identity()
    }
    fn set_wavelength_nm(&mut self, nm: f64) -> Result<f64, String> {
        self.set_wavelength_nm(nm)
    }
    fn snapshot(&mut self) -> Result<MeterSnapshot, String> {
        self.snapshot()
    }
    fn read_power(&mut self) -> Result<PowerSample, String> {
        self.read_power()
    }
    fn take_exchanges(&mut self) -> Vec<DriverExchange> {
        self.take_exchanges()
    }
}

pub(super) struct NativeMeter<D: Driver = ThorlabsMeter> {
    driver: D,
    config: Config,
    identity: Value,
    baseline: MeterSnapshot,
    failed: bool,
}

impl NativeMeter {
    pub fn connect(config: Config) -> Result<Self, String> {
        config.validate(false)?;
        let driver =
            ThorlabsMeter::connect(&resources::normalize(&config.resource)?, DRIVER_TIMEOUT)?;
        Self::initialize(driver, config)
    }
}

impl<D: Driver> NativeMeter<D> {
    fn initialize(mut driver: D, config: Config) -> Result<Self, String> {
        config.validate(false)?;
        let model = driver.identity().model.trim().to_ascii_uppercase();
        if !["PM100USB", "PM100D"].contains(&model.as_str()) {
            return Err(format!(
                "Expected a Thorlabs PM100USB/PM100D, received {model}"
            ));
        }
        driver.set_wavelength_nm(config.wavelength_nm)?;
        let baseline = driver.snapshot()?;
        validate_snapshot(&baseline, &config)?;
        let identity = serde_json::json!({
            "transport": "Thorlabs native driver",
            "driver_timeout_s": DRIVER_TIMEOUT.as_secs_f64(),
            "console": driver.identity(),
            "device_settings": baseline,
            "model_qualification": "driver_identity_query",
            "initialization_exchanges": driver.take_exchanges(),
        });
        Ok(Self {
            driver,
            config,
            identity,
            baseline,
            failed: false,
        })
    }
}

fn validate_snapshot(snapshot: &MeterSnapshot, config: &Config) -> Result<(), String> {
    if !snapshot.average_time_s.is_finite()
        || snapshot.average_time_s <= 0.
        || snapshot.average_time_s >= DRIVER_TIMEOUT.as_secs_f64() - 1.
    {
        return Err("Power-meter averaging must be positive and below 9 s for the 25 s read budget; adjust it in the Thorlabs software".into());
    }
    if snapshot.relative_mode {
        return Err("Disable relative/delta power mode to record absolute optical power".into());
    }
    if snapshot.questionable_condition != 0 {
        return Err(format!(
            "Power meter reports questionable data (condition {}); raw reading retained",
            snapshot.questionable_condition
        ));
    }
    if !wavelength_matches(config.wavelength_nm, snapshot.wavelength_nm) {
        return Err("Power-meter wavelength changed; apply settings or reconnect".into());
    }
    Ok(())
}

impl<D: Driver> MeterDevice for NativeMeter<D> {
    fn identity(&self) -> &Value {
        &self.identity
    }

    fn apply_config(&mut self, config: &Config) -> Result<(), String> {
        config.validate(false)?;
        let result = (|| {
            self.driver.set_wavelength_nm(config.wavelength_nm)?;
            let settings = self.driver.snapshot()?;
            validate_snapshot(&settings, config)?;
            if settings.sensor != self.baseline.sensor {
                return Err("Power sensor changed; reconnect to confirm its calibration".into());
            }
            self.config = config.clone();
            Ok(())
        })();
        self.failed |= result.is_err();
        result
    }

    fn read(&mut self) -> Observation {
        let started = utc_now();
        let begin = Instant::now();
        let result = (|| {
            let before = self.driver.snapshot()?;
            validate_snapshot(&before, &self.config)?;
            if before.sensor != self.baseline.sensor {
                return Err("Power sensor changed; reconnect to confirm its calibration".into());
            }
            let sample = self.driver.read_power()?;
            let completed = utc_now();
            let duration_s = begin.elapsed().as_secs_f64();
            let after = self.driver.snapshot()?;
            validate_snapshot(&after, &self.config)?;
            if before.sensor != after.sensor
                || before.power_unit != after.power_unit
                || before.power_unit != sample.native_unit
                || !wavelength_matches(before.wavelength_nm, after.wavelength_nm)
                || before.attenuation_db != after.attenuation_db
                || before.autorange != after.autorange
                || before.average_time_s != after.average_time_s
            {
                return Err(
                    "Power-meter settings changed during acquisition; raw reading retained".into(),
                );
            }
            let corrected_w = sample.meter_w * self.config.correction_factor;
            if !corrected_w.is_finite() || (sample.meter_w != 0. && corrected_w == 0.) {
                return Err(
                    "Corrected optical power is not representable; raw reading retained".into(),
                );
            }
            Ok(Reading {
                native_value: sample.native_value,
                native_unit: match sample.native_unit {
                    PowerUnit::Watt => "W",
                    PowerUnit::Dbm => "dBm",
                }
                .into(),
                meter_w: sample.meter_w,
                corrected_w,
                wavelength_nm: after.wavelength_nm,
                correction_factor: self.config.correction_factor,
                host_started_at_utc: started,
                host_completed_at_utc: completed,
                duration_s,
                simulated: false,
                device_settings: serde_json::json!({"before":before,"after":after}),
            })
        })();
        let exchanges: Vec<_> = self
            .driver
            .take_exchanges()
            .into_iter()
            .map(|event| Exchange {
                command: format!("{}({})", event.api, event.request.as_deref().unwrap_or("")),
                reply: Some(
                    serde_json::json!({"status":event.status,"value":event.reply}).to_string(),
                ),
                error: (event.status < 0)
                    .then(|| format!("Thorlabs driver status {}", event.status)),
            })
            .collect();
        // A rejected data-quality sample stays blank but does not destroy a
        // working session. Actual failed native calls require reconnecting.
        self.failed |= exchanges.iter().any(|event| event.error.is_some());
        Observation { result, exchanges }
    }

    fn is_poisoned(&self) -> bool {
        self.failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use trkr_transport::thorlabs::{SensorIdentity, WavelengthRangeNm};

    struct Fake {
        identity: MeterIdentity,
        snapshots: VecDeque<Result<MeterSnapshot, String>>,
        sample: PowerSample,
        exchanges: Vec<DriverExchange>,
    }
    impl Driver for Fake {
        fn identity(&self) -> &MeterIdentity {
            &self.identity
        }
        fn set_wavelength_nm(&mut self, nm: f64) -> Result<f64, String> {
            Ok(nm)
        }
        fn snapshot(&mut self) -> Result<MeterSnapshot, String> {
            let result = self.snapshots.pop_front().expect("unexpected snapshot");
            if let Err(error) = &result {
                self.exchanges.push(DriverExchange {
                    api: "getWavelength".into(),
                    status: -1,
                    request: None,
                    reply: Some(error.clone()),
                });
            }
            result
        }
        fn read_power(&mut self) -> Result<PowerSample, String> {
            self.exchanges.push(DriverExchange {
                api: "measPower".into(),
                status: 0,
                request: Some("channel=1".into()),
                reply: Some(self.sample.native_value.to_string()),
            });
            Ok(self.sample.clone())
        }
        fn take_exchanges(&mut self) -> Vec<DriverExchange> {
            std::mem::take(&mut self.exchanges)
        }
    }
    fn snapshot() -> MeterSnapshot {
        MeterSnapshot {
            sensor: SensorIdentity {
                name: "S120C".into(),
                serial_number: "test-sensor".into(),
                calibration_message: "test".into(),
                sensor_type: 1,
                sensor_subtype: 1,
                flags: 1,
            },
            wavelength_nm: 800.,
            wavelength_range_nm: WavelengthRangeNm {
                min_nm: 400.,
                max_nm: 1100.,
            },
            power_range_w: 0.01,
            average_time_s: 0.01,
            attenuation_db: 0.,
            autorange: true,
            relative_mode: false,
            questionable_condition: 0,
            power_unit: PowerUnit::Dbm,
        }
    }
    fn meter(before: MeterSnapshot, after: Result<MeterSnapshot, String>) -> NativeMeter<Fake> {
        NativeMeter::initialize(
            Fake {
                identity: MeterIdentity {
                    backend: "TLPMX".into(),
                    resource: "test".into(),
                    model: "PM100D".into(),
                    serial_number: "console".into(),
                    manufacturer: "Thorlabs".into(),
                },
                snapshots: [Ok(snapshot()), Ok(before), after].into(),
                sample: PowerSample {
                    native_value: -10.,
                    native_unit: PowerUnit::Dbm,
                    meter_w: 1e-4,
                },
                exchanges: Vec::new(),
            },
            Config {
                resource: "TLPMX::USB0::0x1313::0x8078::TEST::INSTR".into(),
                wavelength_nm: 800.,
                correction_factor: 2.5,
            },
        )
        .unwrap()
    }

    #[test]
    fn native_power_preserves_dbm_and_applies_correction_once() {
        let mut meter = meter(snapshot(), Ok(snapshot()));
        let observation = meter.read();
        let reading = observation.result.unwrap();
        assert_eq!(reading.native_value, -10.);
        assert_eq!(reading.native_unit, "dBm");
        assert_eq!(reading.meter_w, 1e-4);
        assert_eq!(reading.corrected_w, 2.5e-4);
        assert_eq!(reading.device_settings["after"]["average_time_s"], 0.01);
        assert_eq!(observation.exchanges[0].command, "measPower(channel=1)");
        assert!(
            observation.exchanges[0]
                .reply
                .as_ref()
                .unwrap()
                .contains("-10")
        );
        assert!(meter.driver.exchanges.is_empty());
        assert!(!meter.is_poisoned());
    }

    #[test]
    fn native_power_rejects_changes_and_retains_the_attempted_sample() {
        for change in 0..6 {
            let mut after = snapshot();
            match change {
                0 => after.wavelength_nm = 801.,
                1 => after.relative_mode = true,
                2 => after.questionable_condition = 1,
                3 => after.sensor.serial_number = "replacement".into(),
                4 => after.power_unit = PowerUnit::Watt,
                _ => after.attenuation_db = 3.,
            }
            let mut meter = meter(snapshot(), Ok(after));
            let observation = meter.read();
            assert!(observation.result.is_err(), "case {change}");
            assert_eq!(observation.exchanges.len(), 1);
            assert!(!meter.is_poisoned());
        }
    }

    #[test]
    fn native_power_does_not_measure_invalid_initial_state_or_reuse_failed_sample() {
        let mut before = snapshot();
        before.relative_mode = true;
        let mut invalid = meter(before, Ok(snapshot()));
        let observation = invalid.read();
        assert!(observation.result.is_err());
        assert!(observation.exchanges.is_empty());
        let mut disconnected = meter(snapshot(), Err("driver disconnected".into()));
        let observation = disconnected.read();
        assert!(
            observation
                .result
                .unwrap_err()
                .contains("driver disconnected")
        );
        assert_eq!(observation.exchanges.len(), 2);
        assert!(disconnected.is_poisoned());
    }

    #[test]
    fn transient_questionable_sample_recovers_on_the_same_native_session() {
        let mut questionable = snapshot();
        questionable.questionable_condition = 1;
        let mut meter = meter(snapshot(), Ok(questionable));
        meter
            .driver
            .snapshots
            .extend([Ok(snapshot()), Ok(snapshot())]);
        assert!(meter.read().result.is_err());
        assert!(!meter.is_poisoned());
        assert!(meter.read().result.is_ok());
    }

    #[test]
    fn native_average_can_take_seconds_but_must_fit_the_request_budget() {
        let mut settings = snapshot();
        settings.average_time_s = 2.;
        let config = Config {
            wavelength_nm: 800.,
            ..Default::default()
        };
        assert!(validate_snapshot(&settings, &config).is_ok());
        settings.average_time_s = 10.;
        assert!(
            validate_snapshot(&settings, &config)
                .unwrap_err()
                .contains("read budget")
        );
    }
}
