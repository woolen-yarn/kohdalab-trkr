# Safety Notes

KohdaLab TRKR controls motion stages, scanners, and measurement instruments.
Software checks are useful, but they are not a substitute for mechanical
limits, interlocks, correct wiring, and operator supervision.

## Before Measurement

- Confirm the sample, optics, stages, and scanners are clear to move.
- Confirm coordinate system and origin.
- Confirm travel limits and scan ranges.
- Start with a small range and low point count.
- Verify the output file path before long measurements.
- Keep a manual way to stop motion or disable hardware.

## During Measurement

- Do not disconnect instruments during an active scan.
- Use the GUI Stop action or interrupt the CLI so cleanup can run.
- Watch for unexpected motion, overloads, or unstable readings.
- Stop immediately if the sample, optics, or instruments behave unexpectedly.

## After Measurement

- Confirm stages and scanners returned to the intended position.
- Save the config used for the run.
- Record any manual hardware changes in lab notes.

The software is provided without warranty. Operators are responsible for safe
laboratory practice and for following instrument manuals.
