# Squat counter: how it works and how to tune it

`sensors::start_squat_counter()` feeds the accelerometer, with the sensor's own timestamps, to
`sensors::SquatDetector` (`src/features/sensors/squat.rs`). The module documentation there describes the
algorithm; every constant is a field of `SquatConfig` with a comment on why it has its value. The constants
were first guesses from the physics of a squat and are now tuned on real recordings of one person (below).

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

## Tuned on real recordings (2026-11-20)

Fourteen recordings of one owner (OnePlus, 50 Hz) are in `tests/fixtures/squat/real_*.csv`, each with a
`# source:` line for the grip. Tuning was deliberately generous: a cheat that gets through costs less than
a real squatter who fails. What the real data showed:

* The amplitude thresholds were fine (real lobes peak at 2 to 8 m/s^2). The rejecting rules were the
  gravity vector (a phone at the chest pitches 25 to 45 degrees; the lagging gravity vector turned that
  into fake acceleration, so depth came out at 1 to 1.6 m and was rejected by the 0.9 m limit), the 35 degree
  orientation limit, lobes cleared from the history by noise lobes, and long braking lobes split by a dip.
* Fixes: vertical acceleration is now `|a|` minus a low-pass of `|a|`; the orientation is smoothed from the
  raw vector (a near free fall has no direction); noise lobes are skipped and split lobes joined; limits
  `max_depth_m` 2.0, `max_rotation_deg` 60, new `max_net_turn_deg` 40 (picking up and putting down),
  `min_rep_gap_s` 1.1 (sit-down plus stand-up is one), `bottom_ratio` 0.5 and `edge_ratio` 0.1 (blips and
  putting down), `strong_peak` 1.4 (arm swing is accepted at any depth).

| Recording | Expected | Old defaults | Now |
|---|---|---|---|
| squats_normal_10 | 10 | 1 | 10 |
| squats_fast_10 | 9-10 (see below) | 2 | 9 |
| squats_slow_5 | 8-10 (10 squats in the file) | 0 | 9 |
| squats_deep_5 | 5 | 1 | 5 |
| squats_half_10 | 10-11 (11 cycles in the file) | 7 | 11 |
| squats_onehand_chest_10 | 10 | 0 | 10 |
| squats_onehand_side_10 | 10 | 10 | 10 |
| squats_armswing_10 | 10 | 0 | 10 |
| squats_prayer_10 | 10 | not recorded | 10 |
| sit_stand_5 | 5-7 | 2 | 7 |
| still, shaking, walking | 0 | 0 | 0 |
| arm_pump | 0-3 (known limit) | 0 | 2 |

Where the file disagrees with the owner's label (slow 5 vs 10 slow cycles, fast 10 vs 9, half 10 vs 11) the
`# note:` line in the file says why; confirm these by hand if the label was right after all.

Margins: how far a constant can move before a real recording leaves its range (the tightest ones first):
`down_min` 0.40 (below 0.38 is fine, above 0.40 loses onehand_side: its weakest first lobe is about 0.4, lowering
it to 0.3 counts extra reps there), `max_net_turn_deg` 40 (35 loses armswing, 42.5 counts a put-down),
`min_rep_gap_s` 1.1 (1.05 counts sit-stand twice, 1.2 loses half), `max_rotation_deg` 60 (54 loses armswing,
63 counts arm_pump), `min_cycle_s` 0.8 (0.84 loses armswing), `bottom_ratio` 0.5 (0.32 to 0.64),
`edge_ratio` 0.1 (0.08 to 0.20), `min_depth_m` 0.2 (0.19 to 0.46), `max_depth_m` 2.0 (down to 1.47),
`max_cycle_s` 5 (down to 4.3), `max_peak` 15 (down to 8.5), `up_min` and `top_brake_min` wide. Several are
thin: more recordings (other people, other grips) are the way to widen them.

Known limits, counted on purpose: moving the phone up and down with the arms (`arm_pump_straight_10`, and
2 of real arm_pump), pumping with a turning wrist (`arm_pump_rotating_10`, turns less than 60 degrees as
measured), and the pick-up or put-down of the phone when it does not turn far.
