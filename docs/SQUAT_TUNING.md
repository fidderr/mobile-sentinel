# Squat counter: how it works and how to tune it

`sensors::start_squat_counter()` feeds the accelerometer, with the sensor's own timestamps, to
`sensors::SquatDetector` (`src/features/sensors/squat.rs`). The module documentation there describes the
algorithm; every constant is a field of `SquatConfig` with a comment on why it has its value. The constants
are first guesses from the physics of a squat, to be tuned with real recordings.

## Record

```rust
use mobile_sentinel::sensors;
sensors::start_squat_counter();                       // or start_accelerometer()
sensors::start_recording("/data/data/<app>/files/squat.csv");
// ... do the movements ...
let samples = sensors::stop_recording();
```

The file is `t_ns,x,y,z` (sensor time in nanoseconds, m/s^2), one line per sample, about 50 per second,
under `#` comment lines. Recording is for debugging: it writes at the sensor rate until stopped.

## Tune

1. Copy the file to `tests/fixtures/squat/<name>.csv`.
2. Add a line `# expect: 10` (the true number of squats; `# expect: 0` for a fake attempt such as walking,
   shaking or pumping the arms; `# expect: 9-11` for a range).
3. `cargo test --all-features --test squat_fixtures` runs every file in the folder and prints expected and
   counted per file. Change `SquatConfig::default()` until all agree, including the synthetic files.

The synthetic fixtures are written by `tests/squat_fixtures.rs` (`UPDATE_FIXTURES=1` regenerates them).
`arm_pump_straight_10` documents the known limit: moving the phone straight up and down with the arms,
without turning it, is counted.
