//! Explicit compatibility revisions for wire and recording contracts.
//!
//! These contracts predate version fields. Revision 1 therefore describes the
//! currently accepted/emitted unversioned shapes. Do not add a field to an
//! existing command, event, CSV row, or metadata file when revising this;
//! introduce an opt-in revision at its boundary instead.

/// JSON-lines commands and events emitted by `--stdio`.
pub(crate) const JSON_LINES_API_REVISION: u32 = 1;

/// User JSON profiles accepted by `Settings::load` after compatibility aliases
/// are normalized in memory.
pub(crate) const PROFILE_REVISION: u32 = crate::config::compatibility::PROFILE_REVISION;

/// CSV columns and adjacent `.run.json` metadata emitted by measurements.
pub(crate) const RECORDING_REVISION: u32 = 1;
