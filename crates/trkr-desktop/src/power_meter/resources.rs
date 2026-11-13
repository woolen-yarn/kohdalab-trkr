//! Resource selection keeps the driver explicit while identifying a physical
//! USB device independently of the driver used to reach it.
use trkr_transport::thorlabs;

pub fn normalize(resource: &str) -> Result<String, String> {
    let resource = resource.trim();
    if thorlabs::is_driver_resource(resource) {
        thorlabs::normalize_resource(resource)
    } else {
        trkr_transport::normalize_usb_resource(resource)
    }
}

pub fn device_key(resource: &str) -> Result<String, String> {
    let normalized = normalize(resource)?;
    let native = normalized
        .strip_prefix(thorlabs::TLPMX_PREFIX)
        .or_else(|| normalized.strip_prefix(thorlabs::TLPM_PREFIX))
        .unwrap_or(&normalized);
    Ok(trkr_transport::normalize_usb_resource(native).unwrap_or_else(|_| native.to_owned()))
}

pub struct Discovery {
    pub resources: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn discover(current_resource: &str) -> Result<Discovery, String> {
    discover_with(current_resource, |backend| match backend {
        Backend::Tlpmx => thorlabs::resources_for(thorlabs::TLPMX_PREFIX),
        Backend::Tlpm => thorlabs::resources_for(thorlabs::TLPM_PREFIX),
        Backend::Native => thorlabs::resources(),
        Backend::Visa => trkr_transport::available_usb_resources(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Tlpmx,
    Tlpm,
    Native,
    Visa,
}

fn discover_with(
    current: &str,
    mut search: impl FnMut(Backend) -> Result<Vec<String>, String>,
) -> Result<Discovery, String> {
    let current = current.trim();
    let selected = if current.starts_with(thorlabs::TLPMX_PREFIX) {
        Some(("TLPMX", Backend::Tlpmx))
    } else if current.starts_with(thorlabs::TLPM_PREFIX) {
        Some(("TLPM", Backend::Tlpm))
    } else if current.to_ascii_uppercase().starts_with("USB") {
        Some(("VISA", Backend::Visa))
    } else {
        None
    };
    if let Some((name, backend)) = selected {
        return merge(vec![(name, search(backend))]);
    }
    let native = search(Backend::Native);
    if native.as_ref().is_ok_and(|found| !found.is_empty()) {
        return merge(vec![("Thorlabs", native)]);
    }
    merge(vec![("Thorlabs", native), ("VISA", search(Backend::Visa))])
}

fn merge(sources: Vec<(&str, Result<Vec<String>, String>)>) -> Result<Discovery, String> {
    let mut found = Discovery {
        resources: Vec::new(),
        warnings: Vec::new(),
    };
    let mut keys = std::collections::HashSet::new();
    let mut succeeded = false;
    for (backend, result) in sources {
        match result {
            Ok(resources) => {
                succeeded = true;
                for resource in resources {
                    match normalize(&resource)
                        .and_then(|resource| Ok((device_key(&resource)?, resource)))
                    {
                        Ok((key, resource)) => {
                            if keys.insert(key) {
                                found.resources.push(resource);
                            }
                        }
                        Err(error) => found.warnings.push(format!("{backend}: {error}")),
                    }
                }
            }
            Err(error) => found.warnings.push(format!("{backend}: {error}")),
        }
    }
    if !succeeded {
        return Err(found.warnings.join("; "));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    const USB: &str = "USB0::0x1313::0x8078::TEST::INSTR";

    #[test]
    fn selected_driver_refresh_never_probes_other_backends() {
        for (resource, expected) in [
            (format!("TLPMX::{USB}"), Backend::Tlpmx),
            (format!("TLPM::{USB}"), Backend::Tlpm),
            (USB.into(), Backend::Visa),
        ] {
            let mut calls = Vec::new();
            let found = discover_with(&resource, |backend| {
                calls.push(backend);
                Ok(vec![resource.clone()])
            })
            .unwrap();
            assert_eq!(calls, [expected]);
            assert_eq!(found.resources, [resource]);
        }
    }

    #[test]
    fn automatic_discovery_stops_when_native_meter_is_found_and_falls_back_when_missing() {
        let mut calls = Vec::new();
        discover_with("", |backend| {
            calls.push(backend);
            Ok(vec![format!("TLPMX::{USB}")])
        })
        .unwrap();
        assert_eq!(calls, [Backend::Native]);
        calls.clear();
        let found = discover_with("", |backend| {
            calls.push(backend);
            match backend {
                Backend::Native => Err("driver missing".into()),
                Backend::Visa => Ok(vec![USB.into()]),
                _ => unreachable!(),
            }
        })
        .unwrap();
        assert_eq!(calls, [Backend::Native, Backend::Visa]);
        assert_eq!(found.resources, [USB]);
        assert_eq!(found.warnings, ["Thorlabs: driver missing"]);
    }

    #[test]
    fn backend_aliases_cannot_open_the_same_meter_twice() {
        assert_eq!(
            device_key(USB).unwrap(),
            device_key(&format!("TLPMX::{USB}")).unwrap()
        );
        assert_eq!(
            device_key(USB).unwrap(),
            device_key("TLPM::USB0::0x1313::0x8078::TEST::0::INSTR").unwrap()
        );
    }

    #[test]
    fn native_discovery_survives_missing_visa_and_preserves_diagnostic() {
        let native = format!("TLPMX::{USB}");
        let found = merge(vec![
            ("Thorlabs", Ok(vec![native.clone()])),
            ("VISA", Err("not installed".into())),
        ])
        .unwrap();
        assert_eq!(found.resources, [native]);
        assert_eq!(found.warnings, ["VISA: not installed"]);
    }

    #[test]
    fn discovery_prefers_native_and_keeps_empty_search_distinct_from_failure() {
        let native = format!("TLPMX::{USB}");
        let found = merge(vec![
            ("Thorlabs", Ok(vec![native.clone()])),
            ("VISA", Ok(vec![USB.into()])),
        ])
        .unwrap();
        assert_eq!(found.resources, [native]);
        assert!(
            merge(vec![("VISA", Ok(vec![]))])
                .unwrap()
                .resources
                .is_empty()
        );
        assert!(
            merge(vec![
                ("Thorlabs", Err("missing".into())),
                ("VISA", Err("missing".into()))
            ])
            .is_err()
        );
    }
}
