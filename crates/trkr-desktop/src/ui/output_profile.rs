//! Per-workflow measurement output destination (directory, filename and
//! timestamp-suffix policy) and its hydration from saved settings.
use crate::config::UserConfigPaths;
use serde_json::Value;
use trkr_core::Workflow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OutputProfile {
    pub(super) dir: String,
    pub(super) filename: String,
    pub(super) auto_timestamp_suffix: bool,
}
pub(super) fn default_output(workflow: Workflow) -> OutputProfile {
    let filename = workflow.default_filename();
    OutputProfile {
        dir: output_dir(),
        filename: filename.into(),
        auto_timestamp_suffix: true,
    }
}
/// Read both the nested output object written by the GUI and the flat fields
/// written by legacy Python profiles.  Empty nested strings deliberately fall
/// back, while an explicit nested `false` remains authoritative.
pub(super) fn hydrated_output(
    raw: &Value,
    workflow: Workflow,
    fallback: OutputProfile,
) -> OutputProfile {
    let settings = raw.pointer(&format!("/measurements/{}", workflow.key()));
    let output = settings.and_then(|value| value.get("output"));
    let nonempty = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
    };
    let nested = |key: &str| output.and_then(|value| value.get(key));
    OutputProfile {
        dir: nonempty(nested("dir"))
            .or_else(|| nonempty(nested("output_dir")))
            .or_else(|| nonempty(settings.and_then(|value| value.get("output_dir"))))
            .unwrap_or(fallback.dir),
        filename: nonempty(nested("filename"))
            .or_else(|| nonempty(settings.and_then(|value| value.get("filename"))))
            .unwrap_or(fallback.filename),
        auto_timestamp_suffix: nested("auto_timestamp_suffix")
            .and_then(Value::as_bool)
            .or_else(|| {
                settings
                    .and_then(|value| value.get("auto_timestamp_suffix"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(fallback.auto_timestamp_suffix),
    }
}
pub(super) fn output_filename(profile: &OutputProfile, demo: bool) -> String {
    let fallback = "trkr_run";
    let name = profile.filename.trim();
    let name = if name.is_empty() { fallback } else { name };
    if demo && !name.to_ascii_lowercase().starts_with("demo_") {
        format!("demo_{name}")
    } else {
        name.to_owned()
    }
}
fn output_dir() -> String {
    UserConfigPaths::for_current_user()
        .results_dir()
        .display()
        .to_string()
}
