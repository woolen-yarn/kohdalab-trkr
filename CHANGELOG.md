# Changelog

## Unreleased

- Raised the mandatory branch-aware coverage floor to 100% after covering every
  shipped statement and branch, making full coverage a durable CI invariant.
- Bounded supported Python versions to the CI-tested 3.13 and 3.14 releases,
  pinned packaging and audit jobs to Python 3.13, and declared 3.14 metadata.
- Ignored macOS Finder metadata and removed the obsolete Black Dependabot entry.
- Covered every shared lock-in validation branch, including unreadable session
  state, invalid response cardinality, numeric table matching, and wait timing.
- Added GSC-01A coverage proving three malformed microstep responses fail
  closed instead of producing an inferred or stale division value.
- Removed an unreachable snapshot-formatting branch that treated `scan_axis`
  as a float; textual axis values now use the single generic string path.
- Covered plot tick decimation's unaligned endpoint and fail-closed handling of
  rows missing both corrected and raw time-axis values.
- Added direct coverage for common TOML loading, non-mutating configuration
  merging with `None` filtering, and midpoint fallback behavior.
- Added explicit package-version tests for both installed distribution metadata
  and source-tree fallback behavior when metadata is unavailable.
- Raised the mandatory branch-aware project coverage floor from 80% to 82%,
  retaining cross-platform headroom while preventing substantial regression.
- Pinned CI runners to Ubuntu 24.04, macOS 15, and Windows 2025 instead of
  mutable `latest` aliases, including packaging and dependency-audit jobs.
- Expanded CI to an explicit Python 3.13/3.14 matrix on Linux, macOS, and
  Windows, with tests pinning the declared interpreter and platform coverage.
- Removed Black from development dependencies after standardizing on Ruff,
  preventing competing formatter versions and configuration drift.
- Canonically formatted Python source, tests, and scripts with Ruff and added
  formatting checks to CI, contributor guidance, and the pull-request checklist.
- Removed the redundant per-module Mypy allowlist now that strict mode targets
  the whole package tree, eliminating manual registration and drift risk.
- Expanded the default Mypy target from selected subpackages to the complete
  `src/kohdalab` tree, including the package root module in every CI run.
- Enabled Mypy's complete strict mode across every shipped Python module and
  pinned that project-wide invariant in metadata tests.
- Eliminated implicit `Any` returns at configuration, controller-name, GUI,
  and shared-lock boundaries, and made `warn_return_any` mandatory in Mypy.
- Prohibited untyped calls and decorators across Mypy, with an explicit typed
  adapter for IPython's dynamically typed notebook display boundary.
- Completed GUI-main method annotations across live display, motion,
  acquisition, plotting, export, shutdown, and Qt event boundaries.
- Removed the final strict-Mypy exception; every shipped Python module is now
  covered by the source-tree typing invariant and strict annotation checks.
- Continued GUI-main typing across config persistence, experiment creation,
  device commands, worker cleanup, and live-status request/result handling.
- Continued GUI-main typing across log redirection, panel sizing, resource
  discovery, device-choice refresh, shared-port synchronization, and output state.
- Began strict typing of the GUI main module with typed value deduplication,
  window construction, widget/layout setup, signal wiring, and tab transitions.
- Added a source-tree/Mypy coverage invariant so new Python modules cannot
  silently bypass strict typing; the remaining GUI-main exception is explicit.
- Fully typed the shared scanner configuration, actuator metadata, controller
  and serial caches, coordinate guards, initialization, motion, and shutdown.
- Fully typed the shared delay-stage configuration, controller cache, unit
  conversion, range checks, initialization, motion controls, and disconnection.
- Fully typed the shared lock-in VISA factory, configuration and connection
  cache, controller delegation, signal reads, settings, and disconnection paths.
- Fully typed the SHOT-302GS serial boundary, readiness and position waits,
  initialization status, motion controls, speed settings, and response checks.
- Fully typed the GSC01 serial boundary, readiness and position waits,
  initialization status, motion controls, speed settings, and response checks.
- Fully typed the CONEX-CC serial boundary, controller-state transitions,
  motion waits, initialization status, stopping, and homing operations.
- Fully typed the SR7265 VISA boundary, multi-response acquisition, overload
  mapping, automation, settings, and shutdown operations.
- Fully typed the LI5640 VISA boundary, display-pair acquisition, overload
  mapping, remote-control release, settings, and shutdown operations.
- Fully typed the SR5210 VISA boundary, multi-response signal fallback,
  overload mapping, automation, settings, and shutdown operations.
- Added project governance and safety baseline files.
- Added GitHub issue and pull request templates.
- Added CI and Dependabot configuration.
- Added strict config validation and hardware-target preflight checks.
- Added fail-closed controller parsing and simulated transport tests.
- Added branch coverage enforcement, type checking, and dependency audits.
- Added UTC measurement timestamps and provenance sidecars with CSV hashes.
- Added isolated wheel and source-distribution validation.
- Bound connected session handles to immutable instrument definitions and made
  bulk disconnect attempt every device before reporting failures.
- Made delay-stage physical zero deterministic and rejected fractional pulse
  and non-finite motion targets before controller I/O.
- Unified scanner origin precedence across initialization and coordinate
  conversion, with strict finite target and hysteresis validation.
- Added config-time driver/catalog compatibility, unique hardware endpoint,
  and measurement device-reference validation.
- Prevented implicit measurement-output overwrite and made explicit manual
  exports use fsynced temporary files with atomic replacement.
- Propagated instrument and serial-port close failures, while continuing bulk
  disconnects and retaining failed handles for an explicit retry.
- Made connection acquisition transactional: stale handles are closed before
  replacement, and failed initial probes roll back newly opened resources.
- Serialized connection-cache mutations so concurrent sessions cannot create
  duplicate instrument handles or race shared scanner serial ownership.
- Made cached-device reconfiguration atomic and restored the previous software
  and controller settings when reconfiguration or its health probe fails.
- Added per-session ownership for shared cached handles so one session cannot
  close hardware still used by another, and made repeated connects idempotent.
- Pinned each shared hardware lease to an immutable instrument-config snapshot
  and rejected conflicting sessions before they can reconfigure the device.
- Pinned each session's connected-device config, detected nested in-place
  mutations before I/O, and always disconnected through the original target.
- Added idempotent `close()` and context-manager cleanup for `DeviceSession`
  and `Experiment`, preserving body exceptions when cleanup also fails.
- Made `connect_all()` transactional, rolling back only newly acquired leases
  in reverse order and attaching rollback failures to the original error.
- Registered initialization leases before hardware motion, reused the exact
  handle in service calls, and rolled back only newly acquired connections.
- Made connection status reflect wrapper health, treated health-check errors as
  disconnected, and rejected device I/O through stale session handles.
- Added shared per-handle I/O locks so separate sessions cannot interleave
  commands on one VISA or serial connection while unrelated devices stay concurrent.
- Extended mandatory Mypy checks to every GUI module and made worker payloads,
  live status handling, scan plans, and Qt 6 enum usage type-safe.
- Added headless GUI integration coverage for measurement startup, stop
  requests, worker failures, completion cleanup, concurrent-operation guards,
  and scan-plan propagation for every supported measurement mode.
- Added GUI measurement preflight tests for missing devices and invalid scan
  ranges, and reject existing CSV or orphan metadata before starting a worker.
- Added deterministic shutdown integration tests covering measurement drain,
  idempotent close requests, device disconnect success/failure, and final waits
  for every GUI worker thread.
- Added GUI device-command lifecycle coverage for worker reuse, initialization,
  failure recovery, and retries; unexpected device-thread exits now restore the
  controls instead of leaving the command state permanently active.
- Added GUI lifecycle coverage for manual moves, live status, and hardware
  resource discovery, including partial failures and unexpected thread exits.
- Made move completion accept both typed positions and row-shaped position
  payloads, avoiding an attribute error after otherwise successful motion.
- Made GUI config loading roll back after field-application failures, validate
  configs before saving, and synchronize successful saves to the live experiment.
- Disabled and rejected manual row export during acquisition, and converted
  manual-save I/O failures into user-visible errors without discarding rows.
- Added fail-closed GUI measurement-point validation for payload type, index,
  mode, axes, finite targets, and lock-in values; invalid points stop acquisition.
- Made 1D and 2D plots fall back to validated scan targets when measured stage
  positions are temporarily unavailable, while preserving snapshots and heatmaps.
- Hardened CONEX-CC and CONEX-AGAP serial protocols: closed ports, timeouts,
  non-ASCII or malformed replies, unknown/unsafe states, controller errors,
  non-finite positions and targets, and mid-motion disconnects now fail closed.
- Serialized all scanner operations sharing one physical serial port, preventing
  CONEX-AGAP U/V command interleaving outside higher-level session APIs.
- Hardened GSC01, GSC01A, and SHOT-302GS transports against closed ports,
  timeouts, partial/non-ASCII replies, mid-command disconnects, invalid ready
  states, and numeric text embedded in malformed position responses.
- Restricted raw delay-stage axes, pulse targets, and speed parameters to valid
  integers before I/O, and made axis-count/default-axis reconfiguration atomic.
- Hardened SR830, LI5640, SR5210, and SR7265 communication against closed VISA
  sessions, timeouts, transport failures, non-ASCII or malformed replies,
  non-finite values, fractional indexes, and hidden mid-command disconnects.
- Serialized every lock-in wrapper operation and verified each requested setting
  against its device read-back, rejecting mismatches and invalid inputs explicitly.
- Made measurement row counts advance only after CSV serialization succeeds,
  finalized provenance even when close or iterator cleanup fails, and preserved
  primary exceptions with cleanup failures recorded as metadata notes.
- Made manual CSV/metadata publication transactional: failed metadata writes now
  remove new exports or restore the complete previous pair after explicit overwrite.
- Enforced run-metadata point-count and terminal-status invariants, and made initial
  sidecar creation exclusive so concurrent runs cannot silently replace provenance.
- Hardened notebook formatters and live plots against invalid progress counters,
  missing or mistyped axes, malformed move positions, unsupported scanner units,
  non-finite values, and conflicting axis aliases.
- Validate live-plot values before mutating retained history, so a bad callback
  payload cannot poison subsequent notebook visualization updates.
- Added construction-time invariants for public position, lock-in signal/settings,
  live-status, and measurement-point models, including finite numeric values,
  physical ranges, progress counters, and paired scanner value/unit fields.
- Made position-row merging reject fractional pulses and conflicting aliases or
  scanner units instead of truncating or silently accepting the last value.
- Made `Experiment.close()` a successful, idempotent terminal transition;
  hardware reads, moves, connects, measurements, config changes, and context
  re-entry now reject use after close while failed cleanup remains retryable.
- Prevented close or config replacement during active facade operations, required
  boolean lifecycle flags, and exposed defensive config/device-map snapshots so
  callers cannot mutate session state through public views.
- Made the CLI own `Experiment` through deterministic context cleanup, defer final
  success output until cleanup completes, and fail nonzero for incomplete runs.
- Added stable CLI exit-code classes (`0` complete, `1` runtime/cleanup failure,
  `2` usage/config error, `130` interrupt), finite move parsing, axis-specific
  coordinate checks, and cleanup diagnostics for interrupted commands.
- Pinned the build backend and added an explicit source manifest covering release
  documentation, maintained notebooks, tests, verification scripts, and the lockfile
  while excluding platform trash, caches, and compiled Python artifacts.
- Added CI double-build verification for byte-identical wheels and content-identical
  sdists, including exact package manifests, wheel `RECORD` hashes and sizes,
  runtime resources, dependency metadata, console entry points, and lock freshness.
- Added source and tag release preflight checks that keep the project version,
  Git tag, README, ROADMAP, CHANGELOG, and citation release date consistent.
- Hardened CI with immutable action revisions, non-persistent checkout credentials,
  bounded job runtimes, and duplicate-run cancellation that preserves tag checks.
- Made pytest fail on warnings, unknown configuration or markers, unexpected xfail
  success, and missing coverage support; synchronized the pull-request checklist.
- Raised the enforced branch-coverage floor from 65% to 80% and made coverage
  paths portable across local and CI operating systems.
- Enabled Ruff's warning, Bugbear, and miscellaneous-error rules; plotting now
  rejects mismatched value/label lengths instead of silently truncating data.
- Enabled security linting for production and test code, replaced optimization-
  removable runtime assertions, and made ignored VISA-clear failures observable.
- Tightened Mypy with strict equality, redundant-cast detection, and additional
  consistency checks across all API, GUI, interface, and instrument modules.
- Required complete function and generic-container annotations in seven core
  model, metadata, measurement-row, scan, status, and requirement modules.
- Extended complete-annotation enforcement to nine GUI helper, interface protocol,
  and API package-boundary modules, for sixteen strict modules in total.
- Typed coordinate-conversion inputs as read-only string-keyed mappings and added
  the conversion boundary as the seventeenth fully annotated module.
- Fully typed the lock-in device API using the concrete connection wrapper for
  acquisition and the controller protocol for injected hardware/test handles.
- Fully typed delay-stage coordinate conversion, lifecycle services, progress
  callbacks, initialization, reads, and moves as the nineteenth strict module.
- Fully typed scanner connections, reads, moves, software-hysteresis progress,
  and initialization services as the twentieth strict module.
- Extended complete-annotation enforcement to root and subsystem package exports,
  common transport helpers, and lock-in validation, covering 29 modules total.
- Fully typed CLI measurement-point callbacks and configuration routing, making
  the command-line boundary the thirtieth strict module.
- Enforced complete annotations on the shared `Experiment` facade used by CLI,
  GUI, and notebooks, bringing strict coverage to 31 modules.
- Typed the device-session lock boundary by its context-manager contract and
  enforced complete annotations on the ownership/lifecycle session module.
- Typed measurement iterators as streams of validated `MeasurementPoint` values
  and enforced complete annotations on the transactional measurement engine.
- Enforced complete annotations on notebook formatting, movement, and live-plot
  helpers, bringing strict module coverage to 34 modules.
- Enforced complete annotations on configuration loading, normalization,
  compatibility checks, and output settings as the thirty-fifth strict module.
- Typed GUI configuration mappings and numeric fallback inputs, then enforced
  complete annotations on API/legacy config loading and save reconstruction.
- Fully typed Qt worker constructors and slots for measurement, device commands,
  moves, live status, resource discovery, and GUI log streaming.
- Fully typed the GSC-01A logical-zero and stop operations, making it the first
  concrete hardware driver under complete-annotation enforcement.
- Fully typed CONEX-AGAP setup, serial command, readiness, motion wait,
  initialization, stop, and home operations as a strict concrete driver.
- Fully typed the SR830 VISA boundary, signal/overload mappings, automation,
  settings, and shutdown operations under strict annotation enforcement.
