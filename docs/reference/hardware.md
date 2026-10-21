# Hardware and transport

The following models are implemented in the Rust drivers. Protocols are covered
by simulated-I/O tests; this is not a claim that every hardware/firmware
combination has been tested on a physical apparatus. See the
[compatibility audit](compatibility.md) for the verification scope and
legacy maintenance operations that are outside the measurement API.

| Category | Model / family | Connection | Implemented measurement operations |
| --- | --- | --- | --- |
| Lock-in | SR7265 | GPIB | XY/Rθ acquisition, sensitivity, time constant, coupling, slope, AC gain, auto actions |
| Lock-in | SR830 | GPIB | XY/Rθ acquisition, sensitivity, time constant, coupling, slope, auto actions |
| Lock-in | SR5210 | GPIB | XY/Rθ acquisition, sensitivity, time constant, slope, auto actions; AC coupling only |
| Lock-in | LI5640 | GPIB | XY/Rθ acquisition with display restoration, sensitivity, time constant, coupling, slope, auto actions |
| Delay controller | SHOT302GS | Serial | Position readback, absolute moves, microstep readout, initialization/home, normal Stop |
| Delay controller | GSC01 / GSC01A | Serial | Position readback, absolute moves, microstep readout, initialization/home, normal Stop |
| Scanner controller | CONEX-CC | Serial | Position readback, absolute moves, home/initialization, Stop, configurable software hysteresis compensation |
| Scanner controller | CONEX-AGAP | Serial | U/V axis control, position readback, absolute moves, closed-loop initialization and Stop |

Built-in motion presets supply defaults; experiment origins, sample conversion,
direction and limits remain profile settings.

| Stage / actuator preset | Controller | Native travel / unit |
| --- | --- | --- |
| SGSP46-500 | SHOT302GS | 500 mm travel |
| OSMS20-35 | SHOT302GS / GSC01 / GSC01A | 35 mm travel |
| OSMS26-300 | SHOT302GS / GSC01 / GSC01A | 300 mm travel |
| OSMS33-600 | SHOT302GS | 600 mm travel |
| TRA12CC | CONEX-CC | 0–12 mm |
| AG-M100D | CONEX-AGAP | −0.75–+0.75° |

GPIB supports primary addresses 1–30. On Windows, the application can use a
system **64-bit VISA runtime** for the selected `GPIB0::<address>::INSTR` resource.
The separate native USB-GPIB helper supports **NI USB-GPIB-HS+** and an
**already initialized Agilent 82357B**. Drivers and adapter initialization are
not bundled. VISA failures do not trigger a native-adapter fallback. On macOS,
serial devices use their `/dev/cu.*` ports. See
[transport setup and limitations](compatibility.md) and
[native helper details](../../native/README.md).

The demo bundles synthetic CSVs only. Experimental CSVs in a local `demo_csv/`
directory are ignored by Git and never copied into release artifacts.
