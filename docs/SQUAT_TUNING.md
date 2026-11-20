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
  `max_depth_m` 2.0, `max_rotation_deg` 75, new `max_net_turn_deg` 40 (picking up and putting down),
  `min_rep_gap_s` (see round 2), `bottom_ratio` 0.5 and `edge_ratio` 0.1 (blips and
  putting down), `strong_peak` 1.4 (arm swing is accepted at any depth).

| Recording | Expected | Old defaults | Round 1 | Now |
|---|---|---|---|---|
| squats_normal_10 | 10 | 1 | 10 | 10 |
| squats_fast_10 | 9-10 | 2 | 9 | 9 |
| squats_fast_10_take2 | 10 | n/a | 9 | 10 |
| squats_slow_5 | 5-10 | 0 | 9 | 9 |
| squats_slow_5_take2 | 5 | n/a | 6 | 5 |
| squats_deep_5 | 5 | 1 | 5 | 5 |
| squats_half_10 | 10-11 | 7 | 11 | 11 |
| squats_half_10_take2 | 10-11 | n/a | 9 | 11 |
| squats_onehand_chest_10 | 10 | 0 | 10 | 10 |
| squats_onehand_side_10 | 10 | 10 | 10 | 10 |
| squats_armswing_10 | 10-11 | 0 | 10 | 11 |
| squats_prayer_10 | 10 | n/a | 10 | 10 |
| sit_stand_5 | 5-7 | 2 | 7 | 6 |
| still, shaking, walking | 0 | 0 | 0 | 0 |
| arm_pump | 0-5 (known limit) | 0 | 2 | 4 |

Round 2 (owner re-recorded slow, fast and half counting aloud on the round 1 build; the owner's labels were
right): slow squats were counted twice (a second bump 0.5 times the squat length after the count), fast
squats lost a rep to the 60 degree rotation limit (64 degrees measured), and half squats lost a rep to the
1.1 s minimum gap (real reps are 1.0 s apart in a fast burst). Fixes: `min_rep_gap_s` 0.8 plus the relative
`min_gap_ratio` 0.58 (a count must be at least 0.58 of the previous interval after the previous one) and
`max_rotation_deg` 75. Ranges that remain, all at the start or end of a recording: `slow_5` (first take: the
file holds 10 equally spaced 4 s cycles against the owner's 5, unexplained; take 2 is exact), `fast_10` (the
10th rep overlaps the pick-up), `half_10` and `half_10_take2` (a shallow first rep, a put-down at the end),
`armswing_10` (a shallow first cycle after pick-up).

Margins (all recordings stay in range): `min_rep_gap_s` 0.5 to 1.0, `min_gap_ratio` 0.55 to 0.6 (0.5 counts a
slow squat twice, 0.65 loses a half rep), `max_rotation_deg` 68 to 90, and from round 1 `down_min` 0.4 (0.3
counts extra), `max_net_turn_deg` 40 (35 to 42), `min_cycle_s` 0.8 (to 0.84), `bottom_ratio` 0.32 to 0.64,
`edge_ratio` 0.08 to 0.20, `min_depth_m` 0.19 to 0.46, `max_depth_m` down to 1.47, `max_cycle_s` down to 4.3,
`max_peak` down to 8.5. Several are thin: more recordings (other people, other grips) widen them.

Known limits, counted on purpose: moving the phone up and down with the arms (`arm_pump_straight_10`, and
2 of real arm_pump), pumping with a turning wrist (`arm_pump_rotating_10`, turns less than 60 degrees as
measured), and the pick-up or put-down of the phone when it does not turn far.
