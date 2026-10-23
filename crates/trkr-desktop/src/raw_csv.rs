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
