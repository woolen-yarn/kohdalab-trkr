//! Explicit compatibility revisions for wire and recording contracts.
//!
//! Profiles and JSON-lines retain revision 1. Recording revision 2 explicitly
//! replaces voltage rounding with lossless f64 text and adds provenance
//! sidecars. Revision 3 exports all available coordinates and distinguishes
//! measured U/V from commanded targets. Revision 4 records acquisition windows
//! separately from storage timing. CSV columns/readers remain supported.
//! Revision 5 separates analysis/raw CSV; revision 6 retains only the measured
//! LI5640 pair. Revision 7 formats delay CSV coordinates using observed pulse
//! quantization while preserving unrounded derived coordinates in raw CSV.

/// JSON-lines commands and events emitted by `--stdio`.
#[cfg(test)]
pub(crate) const JSON_LINES_API_REVISION: u32 = 1;

/// User JSON profiles accepted by `Settings::load` after compatibility aliases
/// are normalized in memory.
#[cfg(test)]
pub(crate) const PROFILE_REVISION: u32 = crate::config::compatibility::PROFILE_REVISION;

/// CSV columns and adjacent `.run.json` metadata emitted by measurements.
pub(crate) const RECORDING_REVISION: u32 = 7;
