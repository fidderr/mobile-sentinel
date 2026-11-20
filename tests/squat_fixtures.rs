//! The squat detector against accelerometer traces.
//!
//! * Every `*.csv` in `tests/fixtures/squat/` is run through the detector and its count compared
//!   with the file's `# expect: N` (or `# expect: A-B`) line. Real recordings made with
//!   `sensors::start_recording` (devtest: `squat_record.sh`) go in the same folder: add the
//!   expect line, tune `SquatConfig`, run `cargo test --all-features --test squat_fixtures`.
//! * The synthetic traces in that folder are generated here (`generated_fixtures`), so they are
//!   documented and reproducible. `UPDATE_FIXTURES=1 cargo test --all-features --test
//!   squat_fixtures` rewrites them.
//!
//! The synthetic physics: a squat is a raised-cosine movement of depth D (m) and period T (s),
//! so its vertical acceleration is `-(D/2) (2 pi / T)^2 cos(2 pi t / T)`. The phone reads that
//! plus gravity, rotated into the phone's own axes by its orientation, plus 0.03 m/s^2 of sensor
//! noise, at 50 Hz with 2 ms of timestamp jitter.
#![cfg(feature = "sensors")]

use std::f64::consts::PI;
use std::fs;
use std::path::PathBuf;

use mobile_sentinel::sensors::{SquatConfig, SquatDetector};

const G: f64 = 9.80665;
const RATE_HZ: f64 = 50.0;
const DEG: f64 = PI / 180.0;
const T0_NS: i64 = 5_000_000_000_000;

// ---- Generator -------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Roughly normal, mean 0, standard deviation 1.
    fn normal(&mut self) -> f64 {
        (0..12).map(|_| self.next()).sum::<f64>() - 6.0
    }
}

type Vec3 = [f64; 3];

/// Rotate a world vector into the device's axes: `R^T v` with `R = Rz(yaw) Ry(pitch) Rx(roll)`.
fn world_to_device(v: Vec3, roll: f64, pitch: f64, yaw: f64) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let v = [cy * v[0] + sy * v[1], -sy * v[0] + cy * v[1], v[2]];
    let (sp, cp) = pitch.sin_cos();
    let v = [cp * v[0] - sp * v[2], v[1], sp * v[0] + cp * v[2]];
    let (sr, cr) = roll.sin_cos();
    [v[0], cr * v[1] + sr * v[2], -sr * v[1] + cr * v[2]]
}

struct Scene {
    seconds: f64,
    /// Linear acceleration in the world frame at time t (m/s^2), z up.
    accel: Box<dyn Fn(f64) -> Vec3>,
    /// (roll, pitch, yaw) of the phone at time t (radians).
    orientation: Box<dyn Fn(f64) -> Vec3>,
}

fn sample_scene(scene: &Scene, seed: u64) -> Vec<(i64, Vec3)> {
    let mut rng = Rng(seed);
    let n = (scene.seconds * RATE_HZ) as usize;
    (0..n)
        .map(|i| {
            let jitter = (rng.next() - 0.5) * 0.004;
            let t = i as f64 / RATE_HZ + jitter.max(-(i as f64) / RATE_HZ);
            let a = (scene.accel)(t);
            let [roll, pitch, yaw] = (scene.orientation)(t);
            let f = world_to_device([a[0], a[1], a[2] + G], roll, pitch, yaw);
            let noisy = [
                f[0] + 0.03 * rng.normal(),
                f[1] + 0.03 * rng.normal(),
                f[2] + 0.03 * rng.normal(),
            ];
            (T0_NS + (t * 1e9) as i64, noisy)
        })
        .collect()
}

/// Vertical acceleration of `reps` squats of period `period` and depth `depth`, one starting
/// every `period + pause` seconds from `lead` seconds on.
fn squats(reps: usize, period: f64, depth: f64, pause: f64, lead: f64) -> impl Fn(f64) -> f64 {
    move |t| {
        (0..reps)
            .map(|k| {
                let tau = (t - lead - k as f64 * (period + pause)) / period;
                if (0.0..1.0).contains(&tau) {
                    -(depth / 2.0) * (2.0 * PI / period).powi(2) * (2.0 * PI * tau).cos()
                } else {
                    0.0
                }
            })
            .sum()
    }
}

fn squat_scene(reps: usize, period: f64, depth: f64, pause: f64, tilt: Vec3) -> Scene {
    let up = squats(reps, period, depth, pause, 2.0);
    Scene {
        seconds: 2.0 + reps as f64 * (period + pause) + 2.0,
        accel: Box::new(move |t| [0.0, 0.0, up(t)]),
        orientation: Box::new(move |_| tilt),
    }
}

struct Fixture {
    name: &'static str,
    note: &'static str,
    expect: &'static str,
    scene: Scene,
}

fn synthetic_fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "slow_squats_10",
            note: "10 slow squats, 3.5 s each, 0.45 m deep, 1 s pause at the top",
            expect: "10",
            scene: squat_scene(10, 3.5, 0.45, 1.0, [0.0; 3]),
        },
        Fixture {
            name: "fast_squats_10",
            note: "10 fast squats in a row, 1.3 s each, 0.35 m deep, no pause",
            expect: "10",
            scene: squat_scene(10, 1.3, 0.35, 0.0, [0.0; 3]),
        },
        Fixture {
            name: "tilted_squats_10",
            note: "10 squats of 2.5 s, 0.4 m, phone held tilted 40 DEG roll, 25 DEG pitch, 30 DEG yaw",
            expect: "10",
            scene: squat_scene(10, 2.5, 0.4, 0.5, [40.0 * DEG, 25.0 * DEG, 30.0 * DEG]),
        },
        Fixture {
            name: "still_30s",
            note: "phone held still for 30 s, with 0.1 m/s^2 of hand tremor at 8 Hz",
            expect: "0",
            scene: Scene {
                seconds: 30.0,
                accel: Box::new(move |t| [0.1 * (2.0 * PI * 8.0 * t).sin(), 0.0, 0.1 * (2.0 * PI * 7.0 * t).sin()]),
                orientation: Box::new(move |_| [10.0 * DEG, 5.0 * DEG, 0.0]),
            },
        },
        Fixture {
            name: "walking_30s",
            note: "walking for 30 s, 1.9 steps per second, phone in the swinging hand",
            expect: "0",
            scene: Scene {
                seconds: 30.0,
                accel: Box::new(move |t| {
                    [
                        1.0 * (2.0 * PI * 0.95 * t).sin(),
                        1.5 * (2.0 * PI * 1.9 * t + 1.0).sin(),
                        2.5 * (2.0 * PI * 1.9 * t).sin(),
                    ]
                }),
                orientation: Box::new(move |t| [8.0 * DEG * (2.0 * PI * 0.95 * t).sin(), 6.0 * DEG * (2.0 * PI * 1.9 * t).sin(), 0.0]),
            },
        },
        Fixture {
            name: "shaking_10s",
            note: "phone shaken hard at 4 Hz for 10 s (25 m/s^2 peaks) with the wrist turning",
            expect: "0",
            scene: Scene {
                seconds: 14.0,
                accel: Box::new(move |t| {
                    let on = if (2.0..12.0).contains(&t) { 1.0 } else { 0.0 };
                    [
                        on * 18.0 * (2.0 * PI * 4.0 * t).sin(),
                        0.0,
                        on * 25.0 * (2.0 * PI * 4.0 * t + 0.5).sin(),
                    ]
                }),
                orientation: Box::new(move |t| {
                    let on = if (2.0..12.0).contains(&t) { 1.0 } else { 0.0 };
                    [on * 30.0 * DEG * (2.0 * PI * 2.0 * t).sin(), 0.0, 0.0]
                }),
            },
        },
        Fixture {
            name: "arm_pump_rotating_10",
            note: "cheat: 10 up-and-down arm pumps of 1.6 s, 0.5 m, wrist turning 55 DEG each time; must not count",
            expect: "0",
            scene: {
                let up = squats(10, 1.6, 0.5, 0.0, 2.0);
                Scene {
                    seconds: 2.0 + 10.0 * 1.6 + 2.0,
                    accel: Box::new(move |t| [0.0, 0.0, up(t)]),
                    orientation: Box::new(move |t| {
                        let tau = ((t - 2.0) / 1.6).rem_euclid(1.0);
                        let inside = (2.0..18.0).contains(&t);
                        let roll = if inside { 55.0 * DEG * (PI * tau).sin().powi(2) } else { 0.0 };
                        [roll, 0.0, 0.0]
                    }),
                }
            },
        },
        Fixture {
            name: "arm_pump_straight_10",
            note: "KNOWN LIMIT: 10 arm pumps of 1.6 s, 0.5 m, phone not turning, is the same motion as a squat and is counted",
            expect: "10",
            scene: squat_scene(10, 1.6, 0.5, 0.0, [0.0; 3]),
        },
    ]
}

fn fixture_text(f: &Fixture, seed: u64) -> String {
    let mut text = format!(
        "# synthetic, generated by tests/squat_fixtures.rs (UPDATE_FIXTURES=1 rewrites it)\n# note: {}\n# expect: {}\nt_ns,x,y,z\n",
        f.note, f.expect
    );
    for (t, [x, y, z]) in sample_scene(&f.scene, seed) {
        text.push_str(&format!("{t},{x:.3},{y:.3},{z:.3}\n"));
    }
    text
}

// ---- Reading fixtures ------------------------------------------------------------------------

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/squat")
}

type Sample = (f64, f64, f64, f64);

fn parse(text: &str) -> (Option<(u32, u32)>, Vec<Sample>) {
    let mut expect = None;
    let mut samples = Vec::new();
    for line in text.lines() {
        if let Some(comment) = line.strip_prefix('#') {
            if let Some(value) = comment.trim().strip_prefix("expect:") {
                let value = value.trim();
                expect = Some(match value.split_once('-') {
                    Some((a, b)) => (a.trim().parse().unwrap(), b.trim().parse().unwrap()),
                    None => {
                        let n = value.parse().unwrap();
                        (n, n)
                    }
                });
            }
        } else if !line.starts_with("t_ns") && !line.trim().is_empty() {
            let v: Vec<f64> = line.split(',').map(|p| p.trim().parse().unwrap()).collect();
            samples.push((v[0] * 1e-9, v[1], v[2], v[3]));
        }
    }
    (expect, samples)
}

fn count(samples: &[Sample], cfg: SquatConfig) -> u32 {
    let mut detector = SquatDetector::new(cfg);
    for &(t, x, y, z) in samples {
        detector.push(t, x, y, z);
    }
    detector.count()
}

// ---- Tests -----------------------------------------------------------------------------------

#[test]
fn generated_fixtures() {
    let update = std::env::var("UPDATE_FIXTURES").is_ok();
    fs::create_dir_all(fixture_dir()).unwrap();
    for (i, f) in synthetic_fixtures().iter().enumerate() {
        let path = fixture_dir().join(format!("{}.csv", f.name));
        let text = fixture_text(f, 1000 + i as u64);
        if update {
            fs::write(&path, &text).unwrap();
        } else {
            let on_disk = fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert!(
                on_disk == text,
                "{} is out of date; run UPDATE_FIXTURES=1 cargo test --all-features --test squat_fixtures",
                path.display()
            );
        }
    }
}

#[test]
fn every_fixture_gives_its_expected_count() {
    let mut files: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "csv"))
        .collect();
    files.sort();
    assert!(files.len() >= 8, "found only {files:?}");
    let mut wrong = Vec::new();
    for path in &files {
        let (expect, samples) = parse(&fs::read_to_string(path).unwrap());
        let (lo, hi) = expect.unwrap_or_else(|| {
            panic!(
                "{} has no `# expect: N` line; add one to use it as a fixture",
                path.display()
            )
        });
        let got = count(&samples, SquatConfig::default());
        println!(
            "{:<28} expected {lo}-{hi}, counted {got}",
            path.file_name().unwrap().to_string_lossy()
        );
        if !(lo..=hi).contains(&got) {
            wrong.push(format!(
                "{}: expected {lo}-{hi}, counted {got}",
                path.display()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn counts_are_exact_over_tempo_and_depth() {
    let mut wrong = Vec::new();
    for period in [1.2, 1.5, 2.0, 3.0, 4.0] {
        for depth in [0.45, 0.6, 0.8] {
            // 0.8 m in under 2 s needs more than 1 g downwards: the sensor reading reverses, a
            // movement no one makes with a phone in the hand.
            if period < 2.0 && depth > 0.7 {
                continue;
            }
            for pause in [0.0, 0.8] {
                let scene = squat_scene(6, period, depth, pause, [0.0; 3]);
                let samples: Vec<_> = sample_scene(&scene, 7)
                    .into_iter()
                    .map(|(t, [x, y, z])| (t as f64 * 1e-9, x, y, z))
                    .collect();
                let got = count(&samples, SquatConfig::default());
                if got != 6 {
                    wrong.push(format!("T={period} D={depth} pause={pause}: counted {got}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_gap_in_the_samples_keeps_the_count_and_restarts() {
    let scene = squat_scene(3, 2.5, 0.4, 0.5, [0.0; 3]);
    let samples: Vec<_> = sample_scene(&scene, 3)
        .into_iter()
        .map(|(t, [x, y, z])| (t as f64 * 1e-9, x, y, z))
        .collect();
    let mut detector = SquatDetector::default();
    for &(t, x, y, z) in &samples {
        detector.push(t, x, y, z);
    }
    assert_eq!(detector.count(), 3);
    for &(t, x, y, z) in &samples {
        detector.push(t + 100.0, x, y, z);
    }
    assert_eq!(detector.count(), 6);
    detector.reset();
    assert_eq!(detector.count(), 0);
}

fn fixture_samples(name: &str) -> Vec<Sample> {
    parse(&fs::read_to_string(fixture_dir().join(format!("{name}.csv"))).unwrap()).1
}

/// Each guard has to be what rejects its cheat: loosen it and the cheat is counted.
#[test]
fn every_guard_is_needed() {
    let rotating = fixture_samples("arm_pump_rotating_10");
    let loose = SquatConfig {
        max_rotation_deg: 180.0,
        ..SquatConfig::default()
    };
    assert_eq!(count(&rotating, loose), 10, "rotation limit");

    let walking = fixture_samples("walking_30s");
    let loose = SquatConfig {
        min_cycle_s: 0.1,
        min_depth_m: 0.0,
        ..SquatConfig::default()
    };
    assert!(count(&walking, loose) > 5, "cycle time and depth limits");

    // The jolt limit: fast squats peak at about 4 m/s^2, so a limit below that rejects them.
    let fast = fixture_samples("fast_squats_10");
    let strict = SquatConfig {
        max_peak: 3.0,
        ..SquatConfig::default()
    };
    assert_eq!(count(&fast, strict), 0, "jolt limit");
}

/// What `adb emu sensor set acceleration` can do (an app's device scenario): a squat's
/// acceleration set every 0.25 to 0.45 s (the shell's sleep and the adb round trip), the value of
/// that moment held until the next set.
#[test]
fn a_squat_set_by_hand_in_steps_counts() {
    let mut wrong = Vec::new();
    for (period, depth, lo, hi, seed) in [
        (3.0, 0.6, 0.25, 0.45, 1u64),
        (3.0, 0.6, 0.25, 0.45, 2),
        (3.0, 0.6, 0.25, 0.45, 3),
        (3.0, 0.5, 0.2, 0.5, 4),
        (4.0, 0.7, 0.25, 0.45, 5),
        (2.5, 0.5, 0.25, 0.45, 6),
    ] {
        let mut rng = Rng(seed);
        let truth = squats(3, period, depth, 1.0, 2.0);
        let mut sets = vec![(0.0, 0.0)];
        let mut t = 0.0;
        while t < 2.0 + 3.0 * (period + 1.0) + 2.0 {
            t += lo + (hi - lo) * rng.next();
            sets.push((t, truth(t)));
        }
        let scene = Scene {
            seconds: t,
            accel: Box::new(move |t| {
                let held = sets
                    .iter()
                    .rev()
                    .find(|(at, _)| *at <= t)
                    .map_or(0.0, |s| s.1);
                [0.0, 0.0, held]
            }),
            orientation: Box::new(|_| [0.0; 3]),
        };
        let samples: Vec<_> = sample_scene(&scene, 5)
            .into_iter()
            .map(|(t, [x, y, z])| (t as f64 * 1e-9, x, y, z))
            .collect();
        let got = count(&samples, SquatConfig::default());
        // The held steps are a coarse squat: one in three may slip by, none may count twice.
        if !(2..=3).contains(&got) {
            wrong.push(format!(
                "T={period} D={depth} sets {lo}-{hi}: counted {got}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{}",
        wrong.join(
            "
"
        )
    );
}
