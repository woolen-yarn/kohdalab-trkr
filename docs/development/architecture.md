# Native Rust workspace

The maintained TRKR application lives in this workspace. Run from the repository root:

```sh
cargo run --manifest-path Cargo.toml --locked --bin kohdalab-trkr
cargo test --manifest-path Cargo.toml --locked
```

Crates separate the experiment plan, device transport, motion safety and desktop GUI.
See the root README for source-built releases and Notebook access.

## Compatibility contracts

Profiles, `--stdio` JSON-lines commands/events, measurement CSV, and adjacent
`.run.json` metadata are the established unversioned **revision 1** contracts.
The application does not add a schema field to existing user JSON: arbitrary
metadata remains user data. Recognised Python-era profile aliases are
normalised in memory while unknown keys are preserved. A future revision must
be opt-in at its input/output boundary and keep revision-1 input readable.


## Extending devices and measurements

- Add scanner capability flags to `ScannerModel::capabilities` and actuator
  calibration defaults to the actuator descriptor in `trkr-motion`. Keep
  model-specific command syntax in `protocol`; configuration, planning and
  initialization consume capabilities rather than duplicating model checks.
- Add lock-in options, terminators and auto commands to `ModelDescriptor`.
  Keep read conversions and model-specific setting commands in the driver.
  SR7265 sensitivity still depends on the queried input mode; it must not be
  replaced with one static table.
- Add workflow names, ordering, default output names, plot counts and axis
  constraints to `WorkflowDefinition` in `trkr-core`. Frontends consume the
  same definition as plan validation. Distinct acquisition, coordinate/profile
  mappings and plot algorithms still need explicit implementations; the table
  is not a runtime plugin interface.
- Keep scan sequencing in `engine/execution.rs` and compensation planning in
  `MotionRig::plan_move`. Preflight and execution must use the same plan. The
  single instrument-owning worker executes it, preserving cancellation, STOP,
  polling deadlines and serial bus access.
- Add states/errors at their typed source, retaining the legacy string boundary
  until an explicit protocol revision is introduced. Profile migrations belong
  in `config/compatibility.rs`; do not reinterpret unknown user metadata.

Before adding a device, run the fake-bus command-order, cancellation, limits and
readback tests. Before adding a workflow, exercise every axis pair, line entry,
compensation, completion/Stop return and retained plot. Preserve the frozen
Python references and compare all existing CSV/profile outputs, excluding only
generation timestamps. Rust workspace tests, strict Clippy, Notebook client
checks and device-free GUI smoke tests cover these existing contracts.
