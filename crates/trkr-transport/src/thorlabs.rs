//! Native adapter for the official Thorlabs TLPM/TLPMX Windows drivers.
//!
//! The ABI below is taken from Thorlabs' official `Light_Analysis_Examples`
//! repository, commit `c10e6083a714828e1f9555fc9842fdb3761d9b51`, notably
//! `PM_SimpleExample.py` and the `TLPM.py`/`TLPMX.py` ctypes wrappers.  We do
//! not ship a vendor DLL: Windows users install it with the Thorlabs driver.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const TLPMX_PREFIX: &str = "TLPMX::";
pub const TLPM_PREFIX: &str = "TLPM::";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterIdentity {
    pub backend: String,
    pub resource: String,
    pub model: String,
    pub serial_number: String,
    pub manufacturer: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WavelengthRangeNm {
    pub min_nm: f64,
    pub max_nm: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerUnit {
    Watt,
    Dbm,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PowerSample {
    pub native_value: f64,
    pub native_unit: PowerUnit,
    pub meter_w: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensorIdentity {
    pub name: String,
    pub serial_number: String,
    pub calibration_message: String,
    pub sensor_type: i16,
    pub sensor_subtype: i16,
    pub flags: i16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeterSnapshot {
    pub sensor: SensorIdentity,
    pub wavelength_nm: f64,
    pub wavelength_range_nm: WavelengthRangeNm,
    pub power_range_w: f64,
    pub autorange: bool,
    pub average_time_s: f64,
    pub attenuation_db: f64,
    pub relative_mode: bool,
    pub questionable_condition: i16,
    pub power_unit: PowerUnit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverExchange {
    pub api: String,
    pub status: i32,
    /// Value or arguments passed to the vendor API, where applicable.
    pub request: Option<String>,
    /// Value returned by the vendor API, where applicable.
    pub reply: Option<String>,
}

#[cfg(any(windows, test))]
fn power_to_w(native_value: f64, native_unit: PowerUnit) -> Result<f64, String> {
    if !native_value.is_finite() {
        return Err("Thorlabs power response is not finite".into());
    }
    let watts = match native_unit {
        PowerUnit::Watt => native_value,
        PowerUnit::Dbm => 10f64.powf(native_value / 10.0) / 1000.0,
    };
    if native_unit == PowerUnit::Dbm && watts == 0. {
        Err("Thorlabs power conversion underflowed; raw reading retained".into())
    } else if watts.is_finite() {
        Ok(watts)
    } else {
        Err("Thorlabs power conversion is not finite".into())
    }
}

/// Returns true only for resources discovered by this native backend.
pub fn is_driver_resource(resource: &str) -> bool {
    resource.starts_with(TLPMX_PREFIX) || resource.starts_with(TLPM_PREFIX)
}

/// Canonicalize a driver-tagged resource without altering Thorlabs' native
/// descriptor (for example `USB0::0x1313::...::INSTR`).
pub fn normalize_resource(resource: &str) -> Result<String, String> {
    let trimmed = resource.trim();
    let (prefix, native) = if let Some(native) = trimmed.strip_prefix(TLPMX_PREFIX) {
        (TLPMX_PREFIX, native)
    } else if let Some(native) = trimmed.strip_prefix(TLPM_PREFIX) {
        (TLPM_PREFIX, native)
    } else {
        return Err("Thorlabs resource must start with TLPMX:: or TLPM::".into());
    };
    let native = native.trim();
    if native.is_empty()
        || native
            .bytes()
            .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
    {
        return Err("invalid Thorlabs driver resource".into());
    }
    Ok(format!("{prefix}{native}"))
}

#[cfg(not(windows))]
pub fn resources() -> Result<Vec<String>, String> {
    Err("Thorlabs TLPM/TLPMX native driver discovery is supported on Windows only".into())
}

#[cfg(not(windows))]
pub fn resources_for(_prefix: &str) -> Result<Vec<String>, String> {
    resources()
}

#[cfg(not(windows))]
pub struct ThorlabsMeter;

#[cfg(not(windows))]
impl ThorlabsMeter {
    pub fn connect(_resource: &str, _timeout: Duration) -> Result<Self, String> {
        Err("Thorlabs TLPM/TLPMX native driver is supported on Windows only".into())
    }
    pub fn identity(&self) -> &MeterIdentity {
        unreachable!("not constructible off Windows")
    }
    pub fn exchanges(&self) -> &[DriverExchange] {
        &[]
    }
    pub fn take_exchanges(&mut self) -> Vec<DriverExchange> {
        Vec::new()
    }
    pub fn wavelength_range_nm(&mut self) -> Result<WavelengthRangeNm, String> {
        Err("unsupported platform".into())
    }
    pub fn wavelength_nm(&mut self) -> Result<f64, String> {
        Err("unsupported platform".into())
    }
    pub fn set_wavelength_nm(&mut self, _nm: f64) -> Result<f64, String> {
        Err("unsupported platform".into())
    }
    pub fn read_power_w(&mut self) -> Result<f64, String> {
        Err("unsupported platform".into())
    }
    pub fn read_power(&mut self) -> Result<PowerSample, String> {
        Err("unsupported platform".into())
    }
    pub fn snapshot(&mut self) -> Result<MeterSnapshot, String> {
        Err("unsupported platform".into())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use libloading::Library;
    use std::env;
    use std::ffi::{CStr, CString};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    const BUFFER: usize = 256;
    const ATTR_SET: i16 = 0;
    const ATTR_MIN: i16 = 1;
    const ATTR_MAX: i16 = 2;
    const UNIT_WATT: i16 = 0;
    const UNIT_DBM: i16 = 1;
    // Thorlabs documents FindRsrc/GetRsrc* as a driver-managed cache. Keep
    // discovery plus the dependent initialization sequence atomic across UI
    // refreshes and connection attempts.
    static DISCOVERY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn discovery_lock() -> &'static Mutex<()> {
        DISCOVERY_LOCK.get_or_init(|| Mutex::new(()))
    }

    type Init = unsafe extern "C" fn(*const i8, i16, i16, *mut u32) -> i32;
    type Close = unsafe extern "C" fn(u32) -> i32;
    type Find = unsafe extern "C" fn(u32, *mut u32) -> i32;
    type ResourceName = unsafe extern "C" fn(u32, u32, *mut i8) -> i32;
    type GetWavelength = unsafe extern "C" fn(u32, i16, *mut f64) -> i32;
    type SetWavelength = unsafe extern "C" fn(u32, f64) -> i32;
    type Measure = unsafe extern "C" fn(u32, *mut f64) -> i32;
    type GetUnit = unsafe extern "C" fn(u32, *mut i16) -> i32;
    type GetReferenceState = unsafe extern "C" fn(u32, *mut i16) -> i32;
    type ErrorMessage = unsafe extern "C" fn(u32, i32, *mut i8) -> i32;
    type Identification = unsafe extern "C" fn(u32, *mut i8, *mut i8, *mut i8, *mut i8) -> i32;
    type SetTimeout = unsafe extern "C" fn(u32, u32) -> i32;
    type GetTimeout = unsafe extern "C" fn(u32, *mut u32) -> i32;
    type GetWavelengthX = unsafe extern "C" fn(u32, i16, *mut f64, u16) -> i32;
    type SetWavelengthX = unsafe extern "C" fn(u32, f64, u16) -> i32;
    type MeasureX = unsafe extern "C" fn(u32, *mut f64, u16) -> i32;
    type GetUnitX = unsafe extern "C" fn(u32, *mut i16, u16) -> i32;
    type GetReferenceStateX = unsafe extern "C" fn(u32, *mut i16, u16) -> i32;
    type GetScalar = unsafe extern "C" fn(u32, i16, *mut f64) -> i32;
    type GetScalarX = unsafe extern "C" fn(u32, i16, *mut f64, u16) -> i32;
    type ReadRegister = unsafe extern "C" fn(u32, i16, *mut i16) -> i32;
    type SensorInfo =
        unsafe extern "C" fn(u32, *mut i8, *mut i8, *mut i8, *mut i16, *mut i16, *mut i16) -> i32;
    type SensorInfoX = unsafe extern "C" fn(
        u32,
        *mut i8,
        *mut i8,
        *mut i8,
        *mut i16,
        *mut i16,
        *mut i16,
        u16,
    ) -> i32;

    struct Driver {
        // Kept alive for every dynamically loaded driver. `None` is used only
        // by the ABI-dispatch unit tests with fake function pointers.
        _library: Option<Library>,
        tag: &'static str,
        init: Init,
        close: Close,
        find: Find,
        resource_name: ResourceName,
        error: ErrorMessage,
        identification: Identification,
        set_timeout: SetTimeout,
        get_timeout: GetTimeout,
        calls: Calls,
    }
    #[derive(Clone, Copy)]
    enum Calls {
        Tlpm {
            get: GetWavelength,
            set: SetWavelength,
            measure: Measure,
            unit: GetUnit,
            reference: GetReferenceState,
            autorange: GetReferenceState,
            range: GetScalar,
            average: GetScalar,
            attenuation: GetScalar,
            register: ReadRegister,
            sensor: SensorInfo,
        },
        Tlpmx {
            get: GetWavelengthX,
            set: SetWavelengthX,
            measure: MeasureX,
            unit: GetUnitX,
            reference: GetReferenceStateX,
            autorange: GetReferenceStateX,
            range: GetScalarX,
            average: GetScalarX,
            attenuation: GetScalarX,
            register: ReadRegister,
            sensor: SensorInfoX,
        },
    }
    impl Driver {
        unsafe fn required<T: Copy>(library: &Library, symbol: &[u8]) -> Result<T, String> {
            // SAFETY: all names/signatures are from the vendor's ctypes wrappers cited above.
            unsafe {
                library
                    .get::<T>(symbol)
                    .map(|value| *value)
                    .map_err(|e| e.to_string())
            }
        }
        unsafe fn load(path: &Path, x: bool) -> Result<Self, String> {
            // Do not allow a DLL beside the executable/current directory to
            // satisfy a dependency of an otherwise trusted driver DLL.
            let flags = libloading::os::windows::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                | libloading::os::windows::LOAD_LIBRARY_SEARCH_SYSTEM32;
            let library = unsafe { libloading::os::windows::Library::load_with_flags(path, flags) }
                .map(Library::from)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let p = if x { "TLPMX_" } else { "TLPM_" };
            let s = |name: &str| format!("{p}{name}\0").into_bytes();
            let calls = if x {
                Calls::Tlpmx {
                    get: unsafe { Self::required(&library, &s("getWavelength")) }?,
                    set: unsafe { Self::required(&library, &s("setWavelength")) }?,
                    measure: unsafe { Self::required(&library, &s("measPower")) }?,
                    unit: unsafe { Self::required(&library, &s("getPowerUnit")) }?,
                    reference: unsafe { Self::required(&library, &s("getPowerRefState")) }?,
                    autorange: unsafe { Self::required(&library, &s("getPowerAutorange")) }?,
                    range: unsafe { Self::required(&library, &s("getPowerRange")) }?,
                    average: unsafe { Self::required(&library, &s("getAvgTime")) }?,
                    attenuation: unsafe { Self::required(&library, &s("getAttenuation")) }?,
                    register: unsafe { Self::required(&library, &s("readRegister")) }?,
                    sensor: unsafe { Self::required(&library, &s("getSensorInfo")) }?,
                }
            } else {
                Calls::Tlpm {
                    get: unsafe { Self::required(&library, &s("getWavelength")) }?,
                    set: unsafe { Self::required(&library, &s("setWavelength")) }?,
                    measure: unsafe { Self::required(&library, &s("measPower")) }?,
                    unit: unsafe { Self::required(&library, &s("getPowerUnit")) }?,
                    reference: unsafe { Self::required(&library, &s("getPowerRefState")) }?,
                    autorange: unsafe { Self::required(&library, &s("getPowerAutorange")) }?,
                    range: unsafe { Self::required(&library, &s("getPowerRange")) }?,
                    average: unsafe { Self::required(&library, &s("getAvgTime")) }?,
                    attenuation: unsafe { Self::required(&library, &s("getAttenuation")) }?,
                    register: unsafe { Self::required(&library, &s("readRegister")) }?,
                    sensor: unsafe { Self::required(&library, &s("getSensorInfo")) }?,
                }
            };
            Ok(Self {
                init: unsafe { Self::required(&library, &s("init")) }?,
                close: unsafe { Self::required(&library, &s("close")) }?,
                find: unsafe { Self::required(&library, &s("findRsrc")) }?,
                resource_name: unsafe { Self::required(&library, &s("getRsrcName")) }?,
                error: unsafe { Self::required(&library, &s("errorMessage")) }?,
                identification: unsafe { Self::required(&library, &s("identificationQuery")) }?,
                set_timeout: unsafe { Self::required(&library, &s("setTimeoutValue")) }?,
                get_timeout: unsafe { Self::required(&library, &s("getTimeoutValue")) }?,
                _library: Some(library),
                tag: if x { TLPMX_PREFIX } else { TLPM_PREFIX },
                calls,
            })
        }
        fn open_session(
            &self,
            native: &CStr,
            timeout: Duration,
            events: &mut Vec<DriverExchange>,
        ) -> Result<u32, String> {
            let mut session = 0;
            let status = unsafe { (self.init)(native.as_ptr(), 1, 0, &mut session) };
            if let Err(error) = self.check(0, "init(no reset)", status, events) {
                if session != 0 {
                    unsafe {
                        (self.close)(session);
                    }
                }
                return Err(error);
            }
            if session == 0 {
                return Err("Thorlabs driver returned an invalid session".into());
            }
            let timeout_ms = timeout.as_millis().max(1).min(u32::MAX as u128) as u32;
            let configured = (|| {
                self.check(
                    session,
                    "setTimeoutValue",
                    unsafe { (self.set_timeout)(session, timeout_ms) },
                    events,
                )?;
                let mut readback = 0;
                self.check(
                    session,
                    "getTimeoutValue",
                    unsafe { (self.get_timeout)(session, &mut readback) },
                    events,
                )?;
                if readback != timeout_ms {
                    return Err(format!(
                        "Thorlabs timeout readback {readback} ms does not match {timeout_ms} ms"
                    ));
                }
                Ok(())
            })();
            if let Err(error) = configured {
                unsafe {
                    (self.close)(session);
                }
                return Err(error);
            }
            Ok(session)
        }
        fn check(
            &self,
            session: u32,
            api: &str,
            status: i32,
            events: &mut Vec<DriverExchange>,
        ) -> Result<(), String> {
            events.push(DriverExchange {
                api: api.into(),
                status,
                request: None,
                reply: None,
            });
            if status >= 0 {
                return Ok(());
            }
            let mut message = [0i8; BUFFER];
            unsafe {
                (self.error)(session, status, message.as_mut_ptr());
            }
            let detail = unsafe { CStr::from_ptr(message.as_ptr()) }.to_string_lossy();
            Err(format!("{api} failed ({status}): {detail}"))
        }
        fn identify(
            &self,
            session: u32,
            events: &mut Vec<DriverExchange>,
        ) -> Result<(String, String, String), String> {
            let mut manufacturer = [0i8; BUFFER];
            let mut model = [0i8; BUFFER];
            let mut serial = [0i8; BUFFER];
            let mut firmware = [0i8; BUFFER];
            let status = unsafe {
                (self.identification)(
                    session,
                    manufacturer.as_mut_ptr(),
                    model.as_mut_ptr(),
                    serial.as_mut_ptr(),
                    firmware.as_mut_ptr(),
                )
            };
            self.check(session, "identificationQuery", status, events)?;
            let text = |buffer: &[i8]| {
                unsafe { CStr::from_ptr(buffer.as_ptr()) }
                    .to_string_lossy()
                    .trim()
                    .to_owned()
            };
            let manufacturer = text(&manufacturer);
            let model = text(&model);
            let serial = text(&serial);
            let firmware = text(&firmware);
            if let Some(event) = events.last_mut() {
                event.reply = Some(format!(
                    "manufacturer={manufacturer}; model={model}; serial={serial}; firmware={firmware}"
                ));
            }
            Ok((manufacturer, model, serial))
        }
        fn get_wavelength(
            &self,
            session: u32,
            attribute: i16,
            events: &mut Vec<DriverExchange>,
        ) -> Result<f64, String> {
            let mut value = 0.0;
            let status = unsafe {
                match self.calls {
                    Calls::Tlpm { get, .. } => get(session, attribute, &mut value),
                    Calls::Tlpmx { get, .. } => get(session, attribute, &mut value, 1),
                }
            };
            self.check(session, "getWavelength", status, events)?;
            let value = finite("wavelength", value)?;
            if let Some(event) = events.last_mut() {
                event.request = Some(format!("attribute={attribute}"));
                event.reply = Some(format!("{value} nm"));
            }
            Ok(value)
        }
        fn set_wavelength(
            &self,
            session: u32,
            value: f64,
            events: &mut Vec<DriverExchange>,
        ) -> Result<(), String> {
            let status = unsafe {
                match self.calls {
                    Calls::Tlpm { set, .. } => set(session, value),
                    Calls::Tlpmx { set, .. } => set(session, value, 1),
                }
            };
            self.check(session, "setWavelength", status, events)?;
            if let Some(event) = events.last_mut() {
                event.request = Some(format!("{value} nm"));
            }
            Ok(())
        }
        fn power(
            &self,
            session: u32,
            events: &mut Vec<DriverExchange>,
        ) -> Result<PowerSample, String> {
            let mut reference = -1;
            let reference_status = unsafe {
                match self.calls {
                    Calls::Tlpm {
                        reference: get_reference,
                        ..
                    } => get_reference(session, &mut reference),
                    Calls::Tlpmx {
                        reference: get_reference,
                        ..
                    } => get_reference(session, &mut reference, 1),
                }
            };
            self.check(session, "getPowerRefState", reference_status, events)?;
            if let Some(event) = events.last_mut() {
                event.reply = Some(format!("{reference}"));
            }
            if reference != 0 {
                return Err(
                    "Thorlabs delta/reference mode is enabled; refusing a relative power reading"
                        .into(),
                );
            }
            let mut unit = -1;
            let unit_status = unsafe {
                match self.calls {
                    Calls::Tlpm { unit: get_unit, .. } => get_unit(session, &mut unit),
                    Calls::Tlpmx { unit: get_unit, .. } => get_unit(session, &mut unit, 1),
                }
            };
            self.check(session, "getPowerUnit", unit_status, events)?;
            let mut value = 0.0;
            let status = unsafe {
                match self.calls {
                    Calls::Tlpm { measure, .. } => measure(session, &mut value),
                    Calls::Tlpmx { measure, .. } => measure(session, &mut value, 1),
                }
            };
            self.check(session, "measPower", status, events)?;
            if let Some(event) = events.last_mut() {
                event.reply = Some(format!("raw={value}; unit={unit}"));
            }
            let value = finite("power", value)?;
            let native_unit = match unit {
                UNIT_WATT => PowerUnit::Watt,
                UNIT_DBM => PowerUnit::Dbm,
                _ => {
                    return Err(format!(
                        "unsupported Thorlabs power unit {unit}; device setting was not changed"
                    ));
                }
            };
            let watts = power_to_w(value, native_unit.clone())?;
            if let Some(event) = events.last_mut() {
                event.reply = Some(format!(
                    "raw={value} {}; watts={watts} W",
                    if unit == UNIT_WATT { "W" } else { "dBm" }
                ));
            }
            Ok(PowerSample {
                native_value: value,
                native_unit,
                meter_w: watts,
            })
        }
        fn snapshot(
            &self,
            session: u32,
            events: &mut Vec<DriverExchange>,
        ) -> Result<MeterSnapshot, String> {
            let mut range = 0.0;
            let mut average = 0.0;
            let mut attenuation = 0.0;
            let mut reference = 0i16;
            let mut autorange = 0i16;
            let mut questionable = 0i16;
            let mut unit = 0i16;
            let s_range = unsafe {
                match self.calls {
                    Calls::Tlpm { range: r, .. } => r(session, ATTR_SET, &mut range),
                    Calls::Tlpmx { range: r, .. } => r(session, ATTR_SET, &mut range, 1),
                }
            };
            self.check(session, "getPowerRange", s_range, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(range.to_string());
            }
            let s_avg = unsafe {
                match self.calls {
                    Calls::Tlpm { average: a, .. } => a(session, ATTR_SET, &mut average),
                    Calls::Tlpmx { average: a, .. } => a(session, ATTR_SET, &mut average, 1),
                }
            };
            self.check(session, "getAvgTime", s_avg, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(average.to_string());
            }
            let s_att = unsafe {
                match self.calls {
                    Calls::Tlpm { attenuation: a, .. } => a(session, ATTR_SET, &mut attenuation),
                    Calls::Tlpmx { attenuation: a, .. } => {
                        a(session, ATTR_SET, &mut attenuation, 1)
                    }
                }
            };
            self.check(session, "getAttenuation", s_att, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(attenuation.to_string());
            }
            let s_ref = unsafe {
                match self.calls {
                    Calls::Tlpm { reference: r, .. } => r(session, &mut reference),
                    Calls::Tlpmx { reference: r, .. } => r(session, &mut reference, 1),
                }
            };
            self.check(session, "getPowerRefState", s_ref, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(reference.to_string());
            }
            let s_auto = unsafe {
                match self.calls {
                    Calls::Tlpm { autorange: a, .. } => a(session, &mut autorange),
                    Calls::Tlpmx { autorange: a, .. } => a(session, &mut autorange, 1),
                }
            };
            self.check(session, "getPowerAutorange", s_auto, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(autorange.to_string());
            }
            let s_unit = unsafe {
                match self.calls {
                    Calls::Tlpm { unit: u, .. } => u(session, &mut unit),
                    Calls::Tlpmx { unit: u, .. } => u(session, &mut unit, 1),
                }
            };
            self.check(session, "getPowerUnit", s_unit, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(unit.to_string());
            }
            let s_reg = unsafe {
                match self.calls {
                    Calls::Tlpm { register, .. } | Calls::Tlpmx { register, .. } => {
                        register(session, 9, &mut questionable)
                    }
                }
            };
            self.check(session, "readRegister(questionable)", s_reg, events)?;
            if let Some(e) = events.last_mut() {
                e.reply = Some(questionable.to_string());
            }
            let mut name = [0i8; BUFFER];
            let mut serial = [0i8; BUFFER];
            let mut message = [0i8; BUFFER];
            let mut ty = 0i16;
            let mut subtype = 0i16;
            let mut flags = 0i16;
            let sensor_status = unsafe {
                match self.calls {
                    Calls::Tlpm { sensor, .. } => sensor(
                        session,
                        name.as_mut_ptr(),
                        serial.as_mut_ptr(),
                        message.as_mut_ptr(),
                        &mut ty,
                        &mut subtype,
                        &mut flags,
                    ),
                    Calls::Tlpmx { sensor, .. } => sensor(
                        session,
                        name.as_mut_ptr(),
                        serial.as_mut_ptr(),
                        message.as_mut_ptr(),
                        &mut ty,
                        &mut subtype,
                        &mut flags,
                        1,
                    ),
                }
            };
            self.check(session, "getSensorInfo", sensor_status, events)?;
            let text = |buf: &[i8]| {
                unsafe { CStr::from_ptr(buf.as_ptr()) }
                    .to_string_lossy()
                    .trim()
                    .to_owned()
            };
            if let Some(e) = events.last_mut() {
                e.reply = Some(format!(
                    "name={}; serial={}; type={ty}; subtype={subtype}; flags={flags}",
                    text(&name),
                    text(&serial)
                ));
            }
            let power_unit = match unit {
                UNIT_WATT => PowerUnit::Watt,
                UNIT_DBM => PowerUnit::Dbm,
                _ => return Err(format!("unsupported Thorlabs power unit {unit}")),
            };
            let sensor = SensorIdentity {
                name: text(&name),
                serial_number: text(&serial),
                calibration_message: text(&message),
                sensor_type: ty,
                sensor_subtype: subtype,
                flags,
            };
            if sensor.name.is_empty() {
                return Err("Thorlabs driver reported no connected sensor".into());
            }
            if sensor.flags & 0x0001 == 0 {
                return Err(
                    "connected Thorlabs sensor does not report power-measurement capability".into(),
                );
            }
            Ok(MeterSnapshot {
                sensor,
                wavelength_nm: self.get_wavelength(session, ATTR_SET, events)?,
                wavelength_range_nm: WavelengthRangeNm {
                    min_nm: self.get_wavelength(session, ATTR_MIN, events)?,
                    max_nm: self.get_wavelength(session, ATTR_MAX, events)?,
                },
                power_range_w: finite("power range", range)?,
                autorange: autorange != 0,
                average_time_s: finite("average time", average)?,
                attenuation_db: finite("attenuation", attenuation)?,
                relative_mode: reference != 0,
                questionable_condition: questionable,
                power_unit,
            })
        }
    }
    fn finite(label: &str, value: f64) -> Result<f64, String> {
        if value.is_finite() {
            Ok(value)
        } else {
            Err(format!("Thorlabs {label} response is not finite"))
        }
    }
    fn candidates(x: bool) -> Vec<PathBuf> {
        let name = if x { "TLPMX_64.dll" } else { "TLPM_64.dll" };
        let env_key = if x {
            "KOHDALAB_TLPMX_DLL"
        } else {
            "KOHDALAB_TLPM_DLL"
        };
        let mut paths = Vec::new();
        if let Some(path) = env::var_os(env_key).map(PathBuf::from)
            && path.is_absolute()
        {
            paths.push(path);
        }
        if let Some(root) = env::var_os("SystemRoot") {
            paths.push(PathBuf::from(root).join("System32").join(name));
        }
        if let Some(program_files) = env::var_os("ProgramFiles") {
            paths.push(
                PathBuf::from(program_files)
                    .join("IVI Foundation")
                    .join("VISA")
                    .join("Win64")
                    .join("Bin")
                    .join(name),
            );
        }
        paths
    }
    fn load_driver(x: bool) -> Result<Driver, String> {
        let paths = candidates(x);
        let mut errors = Vec::new();
        for path in &paths {
            if !path.is_file() {
                continue;
            }
            match unsafe { Driver::load(path, x) } {
                Ok(driver) => return Ok(driver),
                Err(error) => errors.push(error),
            }
        }
        Err(format!(
            "official {} x64 DLL not found or unusable; install the Thorlabs PM100 driver (checked {}; load errors: {})",
            if x { "TLPMX" } else { "TLPM" },
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            errors.join(" | ")
        ))
    }
    /// Enumerate only the selected installed driver. Do not probe the same USB
    /// device again through a second driver family after it has been found.
    pub fn resources_for(prefix: &str) -> Result<Vec<String>, String> {
        let x = match prefix {
            TLPMX_PREFIX => true,
            TLPM_PREFIX => false,
            _ => return Err("Unknown Thorlabs discovery backend".into()),
        };
        let _guard = discovery_lock()
            .lock()
            .map_err(|_| "Thorlabs discovery lock poisoned")?;
        let driver = load_driver(x)?;
        let mut count = 0;
        let mut events = Vec::new();
        driver.check(
            0,
            "findRsrc",
            unsafe { (driver.find)(0, &mut count) },
            &mut events,
        )?;
        let mut result = Vec::new();
        for index in 0..count {
            let mut raw = [0i8; BUFFER];
            driver.check(
                0,
                "getRsrcName",
                unsafe { (driver.resource_name)(0, index, raw.as_mut_ptr()) },
                &mut events,
            )?;
            let native = unsafe { CStr::from_ptr(raw.as_ptr()) }.to_string_lossy();
            if native.trim().is_empty() {
                return Err(format!(
                    "Thorlabs returned an empty resource at index {index}"
                ));
            }
            result.push(format!("{}{native}", driver.tag));
        }
        Ok(result)
    }

    pub fn resources() -> Result<Vec<String>, String> {
        let mut failures = Vec::new();
        let mut succeeded = false;
        for prefix in [TLPMX_PREFIX, TLPM_PREFIX] {
            match resources_for(prefix) {
                Ok(found) => {
                    succeeded = true;
                    if !found.is_empty() {
                        return Ok(found);
                    }
                }
                Err(error) => failures.push(error),
            }
        }
        if succeeded {
            Ok(Vec::new())
        } else {
            Err(failures.join(" | "))
        }
    }
    pub struct ThorlabsMeter {
        driver: Driver,
        session: u32,
        identity: MeterIdentity,
        events: Vec<DriverExchange>,
        _timeout: Duration,
    }
    impl ThorlabsMeter {
        pub fn connect(resource: &str, timeout: Duration) -> Result<Self, String> {
            if timeout.is_zero() {
                return Err("Thorlabs timeout must be positive".into());
            }
            let resource = normalize_resource(resource)?;
            let x = resource.starts_with(TLPMX_PREFIX);
            let native = resource
                .strip_prefix(if x { TLPMX_PREFIX } else { TLPM_PREFIX })
                .unwrap();
            let native = CString::new(native).map_err(|_| "invalid Thorlabs resource")?;
            let driver = load_driver(x)?;
            Self::connect_opened(driver, resource, x, &native, timeout)
        }
        fn connect_opened(
            driver: Driver,
            resource: String,
            x: bool,
            native: &CStr,
            timeout: Duration,
        ) -> Result<Self, String> {
            let mut events = Vec::new();
            let session = driver.open_session(native, timeout, &mut events)?;
            let (manufacturer, model, serial_number) = match driver.identify(session, &mut events) {
                Ok(identity) => identity,
                Err(error) => {
                    // The session exists but the post-open identity query did
                    // not complete. Close it before returning the error.
                    unsafe {
                        (driver.close)(session);
                    }
                    return Err(error);
                }
            };
            let identity = MeterIdentity {
                backend: if x { "TLPMX" } else { "TLPM" }.into(),
                resource,
                model,
                serial_number,
                manufacturer,
            };
            Ok(Self {
                driver,
                session,
                identity,
                events,
                _timeout: timeout,
            })
        }
        pub fn identity(&self) -> &MeterIdentity {
            &self.identity
        }
        pub fn exchanges(&self) -> &[DriverExchange] {
            &self.events
        }
        /// Drains raw native calls so a long-running live monitor cannot grow
        /// an unbounded in-memory history.
        pub fn take_exchanges(&mut self) -> Vec<DriverExchange> {
            std::mem::take(&mut self.events)
        }
        pub fn wavelength_range_nm(&mut self) -> Result<WavelengthRangeNm, String> {
            let min = self
                .driver
                .get_wavelength(self.session, ATTR_MIN, &mut self.events)?;
            let max = self
                .driver
                .get_wavelength(self.session, ATTR_MAX, &mut self.events)?;
            if min > 0.0 && max >= min {
                Ok(WavelengthRangeNm {
                    min_nm: min,
                    max_nm: max,
                })
            } else {
                Err("invalid Thorlabs wavelength range".into())
            }
        }
        pub fn wavelength_nm(&mut self) -> Result<f64, String> {
            self.driver
                .get_wavelength(self.session, ATTR_SET, &mut self.events)
        }
        pub fn set_wavelength_nm(&mut self, nm: f64) -> Result<f64, String> {
            if !nm.is_finite() || nm <= 0.0 {
                return Err("wavelength must be a finite positive value".into());
            }
            let range = self.wavelength_range_nm()?;
            if nm < range.min_nm || nm > range.max_nm {
                return Err(format!(
                    "wavelength {nm} nm is outside {0}..={1} nm",
                    range.min_nm, range.max_nm
                ));
            }
            self.driver
                .set_wavelength(self.session, nm, &mut self.events)?;
            let readback = self.wavelength_nm()?;
            if (readback - nm).abs() > 1e-6 {
                return Err(format!(
                    "Thorlabs wavelength readback {readback} nm does not match requested {nm} nm"
                ));
            }
            Ok(readback)
        }
        pub fn read_power(&mut self) -> Result<PowerSample, String> {
            self.driver.power(self.session, &mut self.events)
        }
        pub fn read_power_w(&mut self) -> Result<f64, String> {
            Ok(self.read_power()?.meter_w)
        }
        pub fn snapshot(&mut self) -> Result<MeterSnapshot, String> {
            self.driver.snapshot(self.session, &mut self.events)
        }
    }
    impl Drop for ThorlabsMeter {
        fn drop(&mut self) {
            if self.session != 0 {
                unsafe {
                    (self.driver.close)(self.session);
                }
                self.session = 0;
            }
        }
    }

    #[cfg(test)]
    mod abi_tests {
        use super::*;
        use std::sync::atomic::{AtomicI32, AtomicU16, AtomicU32, Ordering};
        use std::sync::{Mutex, OnceLock};

        static TEST_LOCK: Mutex<()> = Mutex::new(());
        static CHANNEL: AtomicU16 = AtomicU16::new(0);
        static CLOSED: AtomicU32 = AtomicU32::new(0);
        static INIT_RESET: AtomicI32 = AtomicI32::new(-1);
        static TIMEOUT: AtomicU32 = AtomicU32::new(0);
        static FIND_CALLS: AtomicU32 = AtomicU32::new(0);
        static INIT_RESOURCE: OnceLock<Mutex<String>> = OnceLock::new();
        fn init_resource() -> &'static Mutex<String> {
            INIT_RESOURCE.get_or_init(|| Mutex::new(String::new()))
        }
        unsafe extern "C" fn init(native: *const i8, _: i16, reset: i16, out: *mut u32) -> i32 {
            let resource = unsafe { CStr::from_ptr(native) }
                .to_string_lossy()
                .into_owned();
            *init_resource().lock().unwrap() = resource;
            INIT_RESET.store(reset as i32, Ordering::SeqCst);
            unsafe { *out = 7 };
            0
        }
        unsafe extern "C" fn init_failure(_: *const i8, _: i16, _: i16, out: *mut u32) -> i32 {
            unsafe { *out = 9 };
            -55
        }
        unsafe extern "C" fn close(_: u32) -> i32 {
            CLOSED.fetch_add(1, Ordering::SeqCst);
            0
        }
        unsafe extern "C" fn find(_: u32, out: *mut u32) -> i32 {
            FIND_CALLS.fetch_add(1, Ordering::SeqCst);
            unsafe { *out = 0 };
            0
        }
        unsafe extern "C" fn resource(_: u32, _: u32, _: *mut i8) -> i32 {
            0
        }
        unsafe extern "C" fn error(_: u32, _: i32, _: *mut i8) -> i32 {
            0
        }
        unsafe fn write_text(out: *mut i8, text: &[u8]) {
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr().cast::<i8>(), out, text.len());
                *out.add(text.len()) = 0;
            }
        }
        unsafe extern "C" fn identification(
            _: u32,
            manufacturer: *mut i8,
            model: *mut i8,
            serial: *mut i8,
            firmware: *mut i8,
        ) -> i32 {
            unsafe {
                write_text(manufacturer, b"Thorlabs");
                write_text(model, b"PM100D");
                write_text(serial, b"P12345");
                write_text(firmware, b"1.2.3");
            }
            0
        }
        unsafe extern "C" fn identification_failure(
            _: u32,
            _: *mut i8,
            _: *mut i8,
            _: *mut i8,
            _: *mut i8,
        ) -> i32 {
            -66
        }
        unsafe extern "C" fn set_timeout(_: u32, value: u32) -> i32 {
            TIMEOUT.store(value, Ordering::SeqCst);
            0
        }
        unsafe extern "C" fn get_timeout(_: u32, out: *mut u32) -> i32 {
            unsafe { *out = TIMEOUT.load(Ordering::SeqCst) };
            0
        }
        unsafe extern "C" fn wl(_: u32, _: i16, out: *mut f64) -> i32 {
            unsafe { *out = 11.0 };
            0
        }
        unsafe extern "C" fn wlx(_: u32, _: i16, out: *mut f64, channel: u16) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            unsafe { *out = 22.0 };
            0
        }
        unsafe extern "C" fn set_wl(_: u32, _: f64) -> i32 {
            0
        }
        unsafe extern "C" fn set_wlx(_: u32, _: f64, channel: u16) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            0
        }
        unsafe extern "C" fn power(_: u32, out: *mut f64) -> i32 {
            unsafe { *out = 1.0 };
            0
        }
        unsafe extern "C" fn powerx(_: u32, out: *mut f64, channel: u16) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            unsafe { *out = 2.0 };
            0
        }
        unsafe extern "C" fn unit(_: u32, out: *mut i16) -> i32 {
            unsafe { *out = UNIT_WATT };
            0
        }
        unsafe extern "C" fn unitx(_: u32, out: *mut i16, channel: u16) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            unsafe { *out = UNIT_WATT };
            0
        }
        unsafe extern "C" fn reference(_: u32, out: *mut i16) -> i32 {
            unsafe { *out = 0 };
            0
        }
        unsafe extern "C" fn referencex(_: u32, out: *mut i16, channel: u16) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            unsafe { *out = 0 };
            0
        }
        unsafe extern "C" fn scalar(_: u32, _: i16, out: *mut f64) -> i32 {
            unsafe { *out = 1.0 };
            0
        }
        unsafe extern "C" fn scalarx(_: u32, _: i16, out: *mut f64, channel: u16) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            unsafe { *out = 1.0 };
            0
        }
        unsafe extern "C" fn register(_: u32, _: i16, out: *mut i16) -> i32 {
            unsafe { *out = 0 };
            0
        }
        unsafe extern "C" fn sensor(
            _: u32,
            _: *mut i8,
            _: *mut i8,
            _: *mut i8,
            _: *mut i16,
            _: *mut i16,
            _: *mut i16,
        ) -> i32 {
            0
        }
        unsafe extern "C" fn sensorx(
            _: u32,
            _: *mut i8,
            _: *mut i8,
            _: *mut i8,
            _: *mut i16,
            _: *mut i16,
            _: *mut i16,
            channel: u16,
        ) -> i32 {
            CHANNEL.store(channel, Ordering::SeqCst);
            0
        }
        fn driver(x: bool) -> Driver {
            Driver {
                _library: None,
                tag: TLPM_PREFIX,
                init,
                close,
                find,
                resource_name: resource,
                error,
                identification,
                set_timeout,
                get_timeout,
                calls: if x {
                    Calls::Tlpmx {
                        get: wlx,
                        set: set_wlx,
                        measure: powerx,
                        unit: unitx,
                        reference: referencex,
                        autorange: referencex,
                        range: scalarx,
                        average: scalarx,
                        attenuation: scalarx,
                        register,
                        sensor: sensorx,
                    }
                } else {
                    Calls::Tlpm {
                        get: wl,
                        set: set_wl,
                        measure: power,
                        unit,
                        reference,
                        autorange: reference,
                        range: scalar,
                        average: scalar,
                        attenuation: scalar,
                        register,
                        sensor,
                    }
                },
            }
        }
        #[test]
        fn dispatches_legacy_and_x_channel_one_and_closes() {
            let _guard = TEST_LOCK.lock().unwrap();
            CHANNEL.store(0, Ordering::SeqCst);
            CLOSED.store(0, Ordering::SeqCst);
            let mut events = Vec::new();
            assert_eq!(
                driver(false)
                    .get_wavelength(7, ATTR_SET, &mut events)
                    .unwrap(),
                11.0
            );
            assert_eq!(
                driver(true)
                    .get_wavelength(7, ATTR_SET, &mut events)
                    .unwrap(),
                22.0
            );
            assert_eq!(CHANNEL.load(Ordering::SeqCst), 1);
            let meter = ThorlabsMeter {
                driver: driver(false),
                session: 7,
                identity: MeterIdentity {
                    backend: "TLPM".into(),
                    resource: "x".into(),
                    model: "x".into(),
                    serial_number: "x".into(),
                    manufacturer: "x".into(),
                },
                events,
                _timeout: Duration::from_millis(100),
            };
            drop(meter);
            assert_eq!(CLOSED.load(Ordering::SeqCst), 1);
            let native = CString::new("TEST").unwrap();
            let driver = driver(false);
            let handle = driver
                .open_session(&native, Duration::from_secs(10), &mut Vec::new())
                .unwrap();
            assert_eq!(handle, 7);
            assert_eq!(TIMEOUT.load(Ordering::SeqCst), 10_000);
            unsafe {
                (driver.close)(handle);
            }
            assert_eq!(INIT_RESET.load(Ordering::SeqCst), 0);
        }
        #[test]
        fn negative_status_is_recorded_before_error() {
            let _guard = TEST_LOCK.lock().unwrap();
            let mut events = Vec::new();
            assert!(driver(false).check(7, "fake", -1, &mut events).is_err());
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].status, -1);
            let mut meter = ThorlabsMeter {
                driver: driver(false),
                session: 0,
                identity: MeterIdentity {
                    backend: "TLPM".into(),
                    resource: "x".into(),
                    model: "x".into(),
                    serial_number: "x".into(),
                    manufacturer: "x".into(),
                },
                events,
                _timeout: Duration::from_millis(1),
            };
            assert_eq!(meter.take_exchanges().len(), 1);
            assert!(meter.exchanges().is_empty());
        }
        #[test]
        fn connect_opens_the_selected_resource_without_enumerating() {
            let _guard = TEST_LOCK.lock().unwrap();
            CLOSED.store(0, Ordering::SeqCst);
            FIND_CALLS.store(0, Ordering::SeqCst);
            let native = CString::new("USB0::0x1313::0x8078::P12345::INSTR").unwrap();
            let meter = ThorlabsMeter::connect_opened(
                driver(true),
                format!("{TLPMX_PREFIX}USB0::0x1313::0x8078::P12345::INSTR"),
                true,
                &native,
                Duration::from_secs(10),
            )
            .unwrap();
            assert_eq!(FIND_CALLS.load(Ordering::SeqCst), 0);
            assert_eq!(INIT_RESET.load(Ordering::SeqCst), 0);
            assert_eq!(
                *init_resource().lock().unwrap(),
                "USB0::0x1313::0x8078::P12345::INSTR"
            );
            assert_eq!(meter.identity().manufacturer, "Thorlabs");
            assert_eq!(meter.identity().model, "PM100D");
            assert_eq!(meter.identity().serial_number, "P12345");
            assert!(
                meter
                    .exchanges()
                    .iter()
                    .any(|event| event.api == "identificationQuery")
            );
            drop(meter);
            assert_eq!(CLOSED.load(Ordering::SeqCst), 1);
        }
        #[test]
        fn connect_closes_a_created_session_after_init_or_identity_error() {
            let _guard = TEST_LOCK.lock().unwrap();
            CLOSED.store(0, Ordering::SeqCst);
            let native = CString::new("USB0::test").unwrap();
            let mut init_error = driver(false);
            init_error.init = init_failure;
            let error = match ThorlabsMeter::connect_opened(
                init_error,
                "TLPM::USB0::test".into(),
                false,
                &native,
                Duration::from_secs(1),
            ) {
                Ok(_) => panic!("init failure must not produce a meter"),
                Err(error) => error,
            };
            assert!(error.contains("init(no reset) failed (-55)"));
            assert_eq!(CLOSED.load(Ordering::SeqCst), 1);

            CLOSED.store(0, Ordering::SeqCst);
            let mut identity_error = driver(false);
            identity_error.identification = identification_failure;
            let error = match ThorlabsMeter::connect_opened(
                identity_error,
                "TLPM::USB0::test".into(),
                false,
                &native,
                Duration::from_secs(1),
            ) {
                Ok(_) => panic!("identity failure must not produce a meter"),
                Err(error) => error,
            };
            assert!(error.contains("identificationQuery failed (-66)"));
            assert_eq!(CLOSED.load(Ordering::SeqCst), 1);
        }
    }
}

#[cfg(windows)]
pub use windows::{ThorlabsMeter, resources, resources_for};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tagged_resource_normalization_is_strict() {
        assert_eq!(
            normalize_resource(" TLPMX::USB0::0x1313::PM100::INSTR ").unwrap(),
            "TLPMX::USB0::0x1313::PM100::INSTR"
        );
        assert!(is_driver_resource("TLPM::USB0::x"));
        assert!(!is_driver_resource("USB0::x"));
        assert!(normalize_resource("TLPMX::").is_err());
        assert!(normalize_resource("USB0::x").is_err());
        assert!(normalize_resource("TLPM::a\nb").is_err());
    }
    #[test]
    fn power_conversion_preserves_watts_and_converts_dbm() {
        assert_eq!(power_to_w(0.25, PowerUnit::Watt).unwrap(), 0.25);
        assert!((power_to_w(0.0, PowerUnit::Dbm).unwrap() - 0.001).abs() < 1e-15);
        assert!(power_to_w(f64::NAN, PowerUnit::Watt).is_err());
        assert!(power_to_w(-10000., PowerUnit::Dbm).is_err());
        assert!(power_to_w(10000., PowerUnit::Dbm).is_err());
        assert_eq!(power_to_w(-1e-12, PowerUnit::Watt).unwrap(), -1e-12);
    }
}
