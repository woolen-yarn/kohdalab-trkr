//! Shared raw_csv_v1 contract for KohdaLab measurement applications.
//! CSV quoting preserves multiline instrument text. Structured residual evidence
//! lives in details_json rather than being discarded or expanding a sparse table.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub const FIELDS: &[&str] = &[
    "run_id",
    "event_index",
    "point_index",
    "reading_index",
    "event_type",
    "device_role",
    "quantity",
    "command",
    "raw_response",
    "value",
    "unit",
    "host_started_at_utc",
    "host_completed_at_utc",
    "started_elapsed_s",
    "completed_elapsed_s",
    "result",
    "error",
    "details_json",
];
pub fn new_run_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub struct RawWriter {
    csv: csv::Writer<File>,
    run_id: String,
    event_index: u64,
}
impl RawWriter {
    pub fn create(path: &Path, run_id: &str) -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut writer = Self {
            csv: csv::Writer::from_writer(file),
            run_id: run_id.into(),
            event_index: 0,
        };
        let result = writer
            .csv
            .write_record(FIELDS)
            .map_err(|e| e.to_string())
            .and_then(|()| writer.flush());
        if let Err(error) = result {
            drop(writer);
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
        Ok(writer)
    }
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    #[cfg(test)]
    pub fn open_readonly_for_test(path: &Path, run_id: &str) -> Result<Self, String> {
        Ok(Self {
            csv: csv::Writer::from_writer(File::open(path).map_err(|e| e.to_string())?),
            run_id: run_id.into(),
            event_index: 0,
        })
    }
    pub fn get_ref(&self) -> &File {
        self.csv.get_ref()
    }
    pub fn flush(&mut self) -> Result<(), String> {
        self.csv.flush().map_err(|e| e.to_string())
    }
    pub fn finish(&mut self) -> Result<(), String> {
        self.flush()?;
        self.get_ref().sync_all().map_err(|e| e.to_string())
    }
    pub fn append(&mut self, event: &Value) -> Result<(), String> {
        let mut event = event.clone();
        // Keep native device coordinates in queryable CSV rows as well as in
        // the original event JSON. This observes a position already obtained
        // by the engine; it never adds a motion-controller query.
        let position_values = raw_position_values(&event);
        if let Some(exchanges) = event
            .as_object_mut()
            .and_then(|v| v.remove("exchanges"))
            .and_then(|v| v.as_array().cloned())
        {
            for exchange in exchanges {
                self.write_event(&json!({"event":"transport_exchange", "point_index":event.get("point_index"),
                    "reading_index":event.get("reading_index"), "device_role":event.get("device_role").and_then(Value::as_str).unwrap_or("lockin"),
                    "command":exchange.get("command"), "raw_response":exchange.get("reply"),
                    "error":exchange.get("error"), "operation":exchange.get("operation"), "port":exchange.get("port"),
                    "timing":event.pointer("/timing/signal").or_else(||event.pointer("/timing/position")).or_else(||event.get("timing")), "timing_scope":"logical_operation_window_not_individual_command"}))?;
            }
        }
        self.write_event(&event)?;
        for value in position_values {
            self.write_event(&value)?;
        }
        self.flush()
    }
    fn write_event(&mut self, event: &Value) -> Result<(), String> {
        let mut details = event
            .as_object()
            .cloned()
            .ok_or("Raw event must be an object")?;
        let mut fields = vec![String::new(); FIELDS.len()];
        fields[0] = self.run_id.clone();
        fields[1] = (self.event_index + 1).to_string();
        let mut take = |index, keys: &[&str]| {
            for key in keys {
                if let Some(value) = details.remove(*key)
                    && !value.is_null()
                {
                    fields[index] = cell(&value);
                    break;
                }
            }
        };
        take(2, &["point_index"]);
        take(3, &["reading_index"]);
        take(4, &["event", "event_type"]);
        take(5, &["device_role", "role"]);
        take(6, &["quantity"]);
        take(7, &["wire_command", "command"]);
        take(8, &["wire_response", "raw_response", "normalized_response"]);
        take(9, &["value", "parsed_value"]);
        take(10, &["unit"]);
        take(15, &["result", "status"]);
        take(16, &["wire_error", "error"]);
        // Exact CSV values are already durable in main.csv; retain raw constituents instead.
        details.remove("csv_values");
        if let Some(Value::Object(mut timing)) = details.remove("timing") {
            for (index, key) in [
                (11, "host_started_at_utc"),
                (12, "host_completed_at_utc"),
                (13, "host_started_elapsed_s"),
                (14, "host_completed_elapsed_s"),
            ] {
                if let Some(value) = timing.remove(key) {
                    fields[index] = cell(&value);
                }
            }
            // Nested position/signal windows and unknown device-clock qualification remain explicit.
            if !timing.is_empty() {
                details.insert("timing".into(), Value::Object(timing));
            }
        }
        if fields[15].is_empty() {
            fields[15] = if fields[16].is_empty() { "ok" } else { "error" }.into();
        }
        if !details.is_empty() {
            fields[17] = serde_json::to_string(&details).map_err(|e| e.to_string())?;
        }
        self.csv.write_record(fields).map_err(|e| e.to_string())?;
        self.event_index += 1;
        Ok(())
    }
}

/// Flatten native motion readbacks that are already attached to an audit
/// event.  The calibrated `physical` values remain in the original JSON; these
/// rows expose the controller-native values without guessing values for absent
/// devices.
fn raw_position_values(event: &Value) -> Vec<Value> {
    let mut values = Vec::new();
    for source in ["position", "final_position", "last_observed_position"] {
        // The reading event repeats the preceding position_read snapshot; do
        // not materialize duplicate scalar rows for those same observations.
        if source == "position" && event["event"] == "reading" {
            continue;
        }
        let Some(position) = event.get(source).and_then(Value::as_object) else {
            continue;
        };
        for (field, role, quantity, unit) in [
            (
                "delay_stage_pulse",
                "delay_stage",
                "delay_stage_pulse",
                "pulse",
            ),
            ("delay_stage_mm", "delay_stage", "delay_stage_mm", "mm"),
            ("x_scanner_mm", "scanner_x", "x_scanner_mm", "mm"),
            ("x_scanner_deg", "scanner_x", "x_scanner_deg", "deg"),
            ("y_scanner_mm", "scanner_y", "y_scanner_mm", "mm"),
            ("y_scanner_deg", "scanner_y", "y_scanner_deg", "deg"),
        ] {
            let Some(value) = position.get(field).filter(|value| !value.is_null()) else {
                continue;
            };
            values.push(json!({
                "event": "position_value",
                "point_index": event.get("point_index"),
                "reading_index": event.get("reading_index"),
                "device_role": role,
                "quantity": quantity,
                "value": value,
                "unit": unit,
                "timing": if source == "position" { event.pointer("/timing/position").or_else(||event.get("timing")) } else { None },
                "position_source": source,
                "value_origin": if field == "delay_stage_mm" { "converted_from_delay_stage_pulse_using_effective_calibration" } else { "controller_readback" },
                "timing_scope": "aggregate_position_read_window_not_individual_device_timestamp",
                "parent_event": event.get("event"),
                "parent_error": event.get("error"),
                "position_qualification": if source == "position" { "queried_during_parent_event" } else { "last_observed_no_extra_query; observation_time_unknown" },
            }));
        }
    }
    values
}
fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(v) => v.clone(),
        _ => value.to_string(),
    }
}

/// Read a fixed file-length snapshot using complete CSV records, including
/// quoted embedded newlines. A torn last record is never copied.
pub fn snapshot(
    source: &Path,
    output: &mut impl Write,
    max_point: usize,
    preserve_attempts: bool,
) -> Result<(), String> {
    let file = File::open(source).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let mut bytes = Vec::new();
    file.take(length)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    // Find RFC4180 record boundaries, respecting escaped double quotes.
    let mut quoted = false;
    let mut end = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if quoted && bytes.get(i + 1) == Some(&b'"') {
                i += 2;
                continue;
            }
            quoted = !quoted;
        }
        if bytes[i] == b'\n' && !quoted {
            end = i + 1;
        }
        i += 1;
    }
    let mut reader = csv::Reader::from_reader(&bytes[..end]);
    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let index = |key| {
        headers
            .iter()
            .position(|v| v == key)
            .ok_or_else(|| format!("Raw CSV missing {key}"))
    };
    let point = index("point_index")?;
    let result = index("result")?;
    let kind = index("event_type")?;
    let details = index("details_json")?;
    let mut writer = csv::Writer::from_writer(output);
    writer.write_record(&headers).map_err(|e| e.to_string())?;
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        if !preserve_attempts {
            if record.get(point).is_some_and(|v| !v.is_empty()) {
                let point = record[point].parse::<usize>().map_err(|e| e.to_string())?;
                let allowed = max_point.saturating_add(usize::from(&record[result] == "error"));
                if point > allowed {
                    continue;
                }
            }
            if matches!(&record[kind], "finished" | "run_finished") {
                let data: Value =
                    serde_json::from_str(&record[details]).map_err(|e| e.to_string())?;
                if data
                    .get("rows")
                    .or_else(|| data.get("points_saved"))
                    .and_then(Value::as_u64)
                    .is_some_and(|n| n > max_point as u64)
                {
                    continue;
                }
            }
        }
        writer.write_record(&record).map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())
}
#[cfg(test)]
pub fn test_events(path: &Path) -> Vec<Value> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    reader
        .records()
        .map(|row| {
            let row = row.unwrap();
            let mut object = if row[17].is_empty() {
                serde_json::Map::new()
            } else {
                serde_json::from_str::<Value>(&row[17])
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .clone()
            };
            object.insert("event".into(), json!(&row[4]));
            object.insert("status".into(), json!(&row[15]));
            for (column, key) in [(2, "point_index"), (3, "reading_index")] {
                if !row[column].is_empty() {
                    object.insert(key.into(), json!(row[column].parse::<u64>().unwrap()));
                }
            }
            if !row[11].is_empty()
                || !row[12].is_empty()
                || !row[13].is_empty()
                || !row[14].is_empty()
            {
                let timing = object
                    .entry("timing")
                    .or_insert(json!({}))
                    .as_object_mut()
                    .unwrap();
                for (column, key) in [(11, "host_started_at_utc"), (12, "host_completed_at_utc")] {
                    if !row[column].is_empty() {
                        timing.insert(key.into(), json!(&row[column]));
                    }
                }
                for (column, key) in [
                    (13, "host_started_elapsed_s"),
                    (14, "host_completed_elapsed_s"),
                ] {
                    if !row[column].is_empty() {
                        timing.insert(key.into(), json!(row[column].parse::<f64>().unwrap()));
                    }
                }
            }
            Value::Object(object)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_csv_roundtrips_multiline_quotes_and_exact_original_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.raw.csv");
        let mut writer = RawWriter::create(&path, "run-test").unwrap();
        let response = "head,\"value\"\r\n1.2300000000001e-9\r\n";
        writer.append(&json!({"event":"transport_exchange","point_index":1,"wire_response":response,"wire_command":"READ?\r\n","role":"meter"})).unwrap();
        writer.finish().unwrap();
        let mut reader = csv::Reader::from_path(&path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        assert_eq!(&record[8], response);
        assert_eq!(&record[7], "READ?\r\n");
        assert_eq!(&record[0], "run-test");
        let mut snapshot_bytes = Vec::new();
        snapshot(&path, &mut snapshot_bytes, 1, false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), snapshot_bytes);
    }
    #[test]
    fn native_motion_values_are_explicit_rows_without_inventing_missing_axes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.raw.csv");
        let mut writer = RawWriter::create(&path, "run-test").unwrap();
        writer
            .append(&json!({
                "event": "position_read",
                "point_index": 7,
                "timing": {"position":{"host_started_elapsed_s": 1.25,"host_completed_elapsed_s":1.5,"host_started_at_utc":"2026-11-04T00:00:00Z"},"signal":null},
                "position": {
                    "physical": [null, null, null],
                    "delay_stage_mm": 12.3456789012345,
                    "delay_stage_pulse": 987654321,
                    "x_scanner_deg": -0.123456789012345,
                    "y_scanner_mm": 4.56789012345678,
                    "x_scanner_mm": null,
                    "y_scanner_deg": null
                }
            }))
            .unwrap();
        writer.finish().unwrap();
        let mut reader = csv::Reader::from_path(&path).unwrap();
        let headers = reader.headers().unwrap().clone();
        let column = |name| headers.iter().position(|value| value == name).unwrap();
        let rows: Vec<_> = reader.records().map(Result::unwrap).collect();
        assert_eq!(rows.len(), 5); // Original event plus four native values.
        let native: Vec<_> = rows
            .iter()
            .filter(|row| &row[column("event_type")] == "position_value")
            .collect();
        assert_eq!(native.len(), 4);
        let field = |quantity: &str| {
            native
                .iter()
                .find(|row| &row[column("quantity")] == quantity)
                .unwrap()
        };
        assert_eq!(&field("delay_stage_pulse")[column("value")], "987654321");
        assert_eq!(&field("delay_stage_pulse")[column("unit")], "pulse");
        assert_eq!(
            &field("delay_stage_mm")[column("value")],
            "12.3456789012345"
        );
        assert_eq!(
            &field("x_scanner_deg")[column("value")],
            "-0.123456789012345"
        );
        assert_eq!(&field("y_scanner_mm")[column("value")], "4.56789012345678");
        assert!(native.iter().all(|row| &row[column("point_index")] == "7"));
        assert!(
            native
                .iter()
                .all(|row| &row[column("started_elapsed_s")] == "1.25")
        );
        assert!(
            native.iter().all(|row| {
                !matches!(&row[column("quantity")], "x_scanner_mm" | "y_scanner_deg")
            })
        );
    }
    #[test]
    fn cached_motion_positions_do_not_fabricate_read_timestamps_or_duplicate_reading_rows() {
        let event = json!({"event":"move","point_index":2,"timing":{"host_started_elapsed_s":10.,"host_completed_elapsed_s":20.},"final_position":{"delay_stage_pulse":0,"y_scanner_deg":0.01234567890123456},"error":"position mismatch"});
        let rows = raw_position_values(&event);
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert!(row["timing"].is_null());
            assert!(
                row["position_qualification"]
                    .as_str()
                    .unwrap()
                    .contains("observation_time_unknown")
            );
            assert_eq!(row["parent_error"], "position mismatch");
        }
        assert!(
            raw_position_values(&json!({"event":"reading","position":{"delay_stage_pulse":1}}))
                .is_empty()
        );
    }
    #[test]
    fn snapshot_drops_torn_multiline_record_and_future_points() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("raw.csv");
        let mut writer = RawWriter::create(&path, "test").unwrap();
        writer
            .append(&json!({"event":"reading","point_index":1,"raw_response":"a\nb"}))
            .unwrap();
        writer
            .append(&json!({"event":"reading","point_index":2,"raw_response":"c\nd"}))
            .unwrap();
        drop(writer);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"test,3,3,,,,,,\"torn\n")
            .unwrap();
        let mut result = Vec::new();
        snapshot(&path, &mut result, 1, false).unwrap();
        let rows: Vec<_> = csv::Reader::from_reader(&result[..]).records().collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0].as_ref().unwrap()[8], "a\nb");
    }
    #[test]
    fn sha256_is_standard_and_run_ids_unique() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bytes");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            file_sha256(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(new_run_id(), new_run_id());
    }
}
