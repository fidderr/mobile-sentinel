//! Squat (repetition) detector: counts slow up-and-down movements of a phone from raw
//! accelerometer samples.
//!
//! Pure and platform independent: feed it `(t, x, y, z)` samples, in seconds and m/s^2 as the
//! Android accelerometer reports them (the reaction to gravity, so a phone lying flat reads
//! `z = +9.8`), and read [`SquatDetector::count`]. On Android the SDK feeds it from the sensor
//! callback with the sensor's own timestamps (see `sensors::start_squat_counter`).
//!
//! # Algorithm
//!
//! 1. The vertical acceleration is the acceleration magnitude minus gravity (a slow low-pass of
//!    the magnitude, `gravity_tau_s`), smoothed over about 0.1 s. Using the magnitude instead of
//!    the part along a gravity vector matters: a phone held at the chest pitches 25 to 45 degrees
//!    in a squat, and a lagging gravity vector turned that into several m/s^2 of fake
//!    acceleration (real recordings, see `docs/SQUAT_TUNING.md`).
//! 2. The smoothed signal is cut into *lobes*: runs of one sign that rise above `lobe_on` and fall
//!    below `lobe_off`. Two lobes of the same sign closer than `merge_gap_s` are one lobe; lobes
//!    too small or short to matter are skipped, and same-sign lobes split by them (less than
//!    `split_gap_s` apart) are joined.
//! 3. One repetition is the lobe sequence negative (speeding up downwards), positive (braking at
//!    the bottom and speeding up upwards), negative (braking at the top). The closing negative
//!    lobe is also the opening lobe of the next repetition, so repetitions without a pause count.
//! 4. A candidate is counted only if:
//!    * the time between the peaks of the two negative lobes is within `min_cycle_s..max_cycle_s`
//!      and at least `min_rep_gap_s` after the previous count (rejects walking, shaking, quick
//!      pumping, and the two halves of sitting down and standing up),
//!    * the lobe peaks reach their minimums, none exceeds `max_peak` (a jolt), the bottom lobe is
//!      at least `bottom_ratio` of the stronger outer lobe and each outer lobe at least
//!      `edge_ratio` of the bottom lobe (rejects blips and putting the phone down),
//!    * the phone's orientation (direction of the smoothed raw vector) never turns more than
//!      `max_rotation_deg` away from its direction at the start, and ends within
//!      `max_net_turn_deg` of it (rejects picking up and putting down; generous on purpose),
//!    * the depth is within `min_depth_m..max_depth_m` (the lower limit is waived when all lobes
//!      reach `strong_peak`): the vertical acceleration is integrated
//!      twice between the start and the top of the movement, where the velocity is zero; the small
//!      velocity left at the end is removed linearly. Only the lower limit does real work.
//!
//! # Known limit
//!
//! Moving the phone straight up and down with the arms, without turning it, produces the same
//! acceleration as a squat and is counted. The detector cannot tell them apart from the phone
//! alone.
//!
//! # Tuning
//!
//! Every constant is in [`SquatConfig`]. They are tuned on real recordings of one person
//! (`tests/fixtures/squat/real_*.csv`), deliberately generously: letting a cheat through costs
//! less than failing someone who squats differently. Real squats peak at 2 to 8 m/s^2, well
//! above the 0.3 to 3 of a smooth movement. Real recordings (see
//! `sensors::start_recording`) dropped into `tests/fixtures/squat/` with an `# expect:` line are
//! checked by `cargo test`, which is how they are meant to be tuned.

use std::collections::VecDeque;

/// All tunable constants of the detector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SquatConfig {
    /// Time constant of the gravity (acceleration magnitude) low-pass (s). Must be well above a
    /// squat's duration, or the estimate follows the movement and flattens it. 1.5 s is best on
    /// the real recordings: 3 s and longer lets sit-downs and arm pumps count, and loses slow
    /// squats to drift.
    pub gravity_tau_s: f64,
    /// Time constant of the vertical acceleration smoothing (s). 0.05 s is a 0.1 s window: it
    /// removes sensor noise and tremor, and keeps the 0.3 s lobes of a fast squat.
    pub smooth_tau_s: f64,
    /// Time constant of the orientation smoothing (s).
    pub direction_tau_s: f64,
    /// A lobe starts when the smoothed acceleration passes this (m/s^2). Above sensor noise
    /// (about 0.05) and hand tremor (about 0.1).
    pub lobe_on: f64,
    /// A lobe ends when it falls below this (m/s^2). Lower than `lobe_on` so noise at the edge
    /// does not chop a lobe up.
    pub lobe_off: f64,
    /// Same-sign lobes closer than this (s) are one lobe (a dip in the middle of a long lobe).
    pub merge_gap_s: f64,
    /// Longest quiet gap (s) between the three lobes of one repetition. A pause at the very top
    /// is not part of it: it comes before the first lobe.
    pub max_gap_s: f64,
    /// A lobe that starts at least this long (s) after the previous one ended starts from rest
    /// (the velocity is zero where it starts). Otherwise it follows a movement straight on.
    pub isolated_gap_s: f64,
    /// Shortest lobe (s). A real squat lobe lasts at least a third of a second; 0.15 s still lets
    /// a very fast squat through, and rejects a twitch.
    pub min_lobe_s: f64,
    /// Smallest peak of the first lobe, speeding up downwards (m/s^2). Real squats peak at 0.6
    /// (the weakest, a sit-stand) to 6; the smooth-movement physics of a 3 s squat gives 0.4.
    pub down_min: f64,
    /// Smallest peak of the middle lobe, the bottom (m/s^2).
    pub up_min: f64,
    /// Smallest peak of the closing lobe, braking at the top (m/s^2). A little lower: people
    /// ease out of the top.
    pub top_brake_min: f64,
    /// Same-sign lobes with only skipped (too small or short) lobes between them, closer than
    /// this (s), are one lobe. Deep squats have the long braking lobe split by a 0.1 s dip.
    pub split_gap_s: f64,
    /// Shortest time (s) between two counts, measured at the closing lobe peaks. Real squats can
    /// follow each other 0.9 to 1.0 s apart (fast bursts of half squats, arm swing).
    pub min_rep_gap_s: f64,
    /// A count must also be at least this fraction of the previous interval (capped at
    /// `max_cycle_s`) after the previous count. A slow squat has a second bump 0.5 times its own
    /// length after the count (counted twice otherwise), sitting down plus standing up 0.3 times;
    /// real changes of pace stay above 0.65. Works from 0.55 to 0.6.
    pub min_gap_ratio: f64,
    /// Each outer lobe must be at least this fraction of the bottom lobe. Putting the phone down
    /// ends in a 10 m/s^2 bottom lobe and a 0.8 m/s^2 closing lobe (0.08); real repetitions are
    /// at least 0.13.
    pub edge_ratio: f64,
    /// The bottom lobe must be at least this fraction of the stronger outer lobe. Blips between
    /// two repetitions are 0.15 to 0.3 of their neighbours; real bottoms are 0.5 and up.
    pub bottom_ratio: f64,
    /// The orientation at the end of a repetition may differ at most this far (degrees) from the
    /// start. Squats come back to the start (up to 37 degrees when the arms swing); picking up and
    /// putting down end 40 degrees or more away.
    pub max_net_turn_deg: f64,
    /// Shortest repetition (s) between the peaks of the two negative lobes. Walking is 0.5 s, a
    /// shake or a quick pump 0.3 to 0.6 s; the quickest real squat is about 1 s.
    pub min_cycle_s: f64,
    /// Longest repetition (s). Slower than this is not a rep, it is lowering the phone.
    pub max_cycle_s: f64,
    /// Smallest depth (m). Blips between repetitions integrate to 0.04 to 0.15 m; real half
    /// squats to 0.27 m and up (the integration overestimates, a real squat is about 0.5 m).
    /// Not applied when all three lobes reach `strong_peak`.
    pub min_depth_m: f64,
    /// A repetition whose three lobes all peak at least this high (m/s^2) is accepted at any
    /// depth. Swinging the arms while squatting moves the phone little vertically (depth 0.03 to
    /// 0.45 m) but the lobes stay at 1.5 and up; walking, the thing a shallow limit is for, has
    /// its weakest lobe at 1.3 or lower.
    pub strong_peak: f64,
    /// Largest depth (m). Real squats integrate to up to 1.6 m (abrupt stops overshoot), so this
    /// only catches a runaway integration.
    pub max_depth_m: f64,
    /// Most the phone's orientation may turn away from its start during a repetition (degrees).
    /// Real squats turn it by 20 to 64 degrees (fast squats with the phone at the chest the
    /// most); picking up and putting down by 80 or more. Arm pumps with a turning wrist stay
    /// below this too: a known limit. Every value from 68 to 90 passes the recordings.
    pub max_rotation_deg: f64,
    /// A lobe peak above this (m/s^2) is a jolt, a drop or a shake, not a squat; it also
    /// discards the lobes seen so far.
    pub max_peak: f64,
    /// A gap between samples longer than this (s) restarts the detector (the count is kept).
    pub gap_reset_s: f64,
}

impl Default for SquatConfig {
    fn default() -> Self {
        Self {
            gravity_tau_s: 1.5,
            smooth_tau_s: 0.05,
            direction_tau_s: 0.1,
            lobe_on: 0.15,
            lobe_off: 0.08,
            merge_gap_s: 0.15,
            max_gap_s: 0.6,
            isolated_gap_s: 0.3,
            min_lobe_s: 0.15,
            down_min: 0.4,
            up_min: 0.4,
            top_brake_min: 0.3,
            max_net_turn_deg: 40.0,
            split_gap_s: 0.2,
            min_rep_gap_s: 0.8,
            min_gap_ratio: 0.58,
            edge_ratio: 0.1,
            bottom_ratio: 0.5,
            min_cycle_s: 0.8,
            max_cycle_s: 5.0,
            min_depth_m: 0.2,
            max_depth_m: 2.0,
            strong_peak: 1.4,
            max_rotation_deg: 75.0,
            max_peak: 15.0,
            gap_reset_s: 1.0,
        }
    }
}

/// One run of acceleration of one sign. `sign` is -1 (down) or +1 (up).
#[derive(Debug, Clone, Copy)]
struct Lobe {
    sign: i8,
    t0: f64,
    t1: f64,
    peak: f64,
    peak_t: f64,
    /// Started from rest: the phone was still before it.
    isolated: bool,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    t: f64,
    av: f64,
    dir: [f64; 3],
}

/// Counts squats from accelerometer samples. See the module documentation.
#[derive(Debug, Clone)]
pub struct SquatDetector {
    cfg: SquatConfig,
    count: u32,
    last_count_t: f64,
    last_interval: f64,
    last_t: Option<f64>,
    /// Slow low-pass of the acceleration magnitude: gravity, about 9.8.
    gravity: f64,
    dir_sum: [f64; 3],
    av: f64,
    active: Option<Lobe>,
    pending: Option<Lobe>,
    prev_end: f64,
    history: Vec<Lobe>,
    buffer: VecDeque<Sample>,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    if n > 1e-9 {
        [a[0] / n, a[1] / n, a[2] / n]
    } else {
        [0.0, 0.0, 1.0]
    }
}

impl Default for SquatDetector {
    fn default() -> Self {
        Self::new(SquatConfig::default())
    }
}

impl SquatDetector {
    pub fn new(cfg: SquatConfig) -> Self {
        Self {
            cfg,
            count: 0,
            last_count_t: f64::NEG_INFINITY,
            last_interval: 0.0,
            last_t: None,
            gravity: 9.81,
            dir_sum: [0.0; 3],
            av: 0.0,
            active: None,
            pending: None,
            prev_end: f64::NEG_INFINITY,
            history: Vec::new(),
            buffer: VecDeque::new(),
        }
    }

    /// Squats counted since creation or the last [`reset`](Self::reset).
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Count back to zero and forget the signal so far.
    pub fn reset(&mut self) {
        self.count = 0;
        self.restart();
    }

    fn restart(&mut self) {
        self.last_t = None;
        self.last_count_t = f64::NEG_INFINITY;
        self.last_interval = 0.0;
        self.active = None;
        self.pending = None;
        self.prev_end = f64::NEG_INFINITY;
        self.history.clear();
        self.buffer.clear();
        self.av = 0.0;
    }

    /// Feed one sample (`t` in seconds, x/y/z in m/s^2). Returns true when this sample completed
    /// a squat.
    pub fn push(&mut self, t: f64, x: f64, y: f64, z: f64) -> bool {
        let a = [x, y, z];
        let Some(last) = self.last_t else {
            self.last_t = Some(t);
            self.gravity = norm(a);
            self.dir_sum = unit(a);
            return false;
        };
        let dt = t - last;
        if dt <= 0.0 {
            return false;
        }
        if dt > self.cfg.gap_reset_s {
            self.restart();
            return self.push(t, x, y, z);
        }
        self.last_t = Some(t);

        let kg = 1.0 - (-dt / self.cfg.gravity_tau_s).exp();
        let ks = 1.0 - (-dt / self.cfg.smooth_tau_s).exp();
        let kd = 1.0 - (-dt / self.cfg.direction_tau_s).exp();
        let mag = norm(a);
        self.gravity += kg * (mag - self.gravity);
        if self.gravity < 1.0 {
            // Free fall or a broken sensor: nothing to measure against.
            return false;
        }
        let vertical = mag - self.gravity;
        self.av += ks * (vertical - self.av);
        // The raw vector, not its unit vector: in a near free fall the direction is noise and
        // must carry little weight.
        for (sum, raw) in self.dir_sum.iter_mut().zip(a) {
            *sum += kd * (raw - *sum);
        }

        self.buffer.push_back(Sample {
            t,
            av: self.av,
            dir: unit(self.dir_sum),
        });
        let keep = self.cfg.max_cycle_s + 3.0;
        while self.buffer.front().is_some_and(|s| t - s.t > keep) {
            self.buffer.pop_front();
        }

        self.track_lobes(t)
    }

    fn track_lobes(&mut self, t: f64) -> bool {
        let cfg = self.cfg;
        let v = self.av;
        let mut counted = false;

        if let Some(p) = self.pending {
            if t - p.t1 > cfg.merge_gap_s {
                self.pending = None;
                counted |= self.finish(p);
            }
        }
        if let Some(l) = self.active.as_mut() {
            let signed = f64::from(l.sign) * v;
            if signed < cfg.lobe_off {
                l.t1 = t;
                self.prev_end = t;
                self.pending = self.active.take();
            } else if signed > l.peak {
                l.peak = signed;
                l.peak_t = t;
            }
        }
        if self.active.is_none() && v.abs() > cfg.lobe_on {
            let sign: i8 = if v > 0.0 { 1 } else { -1 };
            match self.pending.take() {
                Some(p) if p.sign == sign => self.active = Some(p),
                other => {
                    if let Some(p) = other {
                        counted |= self.finish(p);
                    }
                    self.active = Some(Lobe {
                        sign,
                        t0: t,
                        t1: t,
                        peak: v.abs(),
                        peak_t: t,
                        isolated: t - self.prev_end >= cfg.isolated_gap_s,
                    });
                }
            }
        }
        counted
    }

    /// A lobe is complete. Returns true if it completed a squat.
    fn finish(&mut self, lobe: Lobe) -> bool {
        let cfg = self.cfg;
        if lobe.peak > cfg.max_peak {
            self.history.clear();
            return false;
        }
        if lobe.t1 - lobe.t0 < cfg.min_lobe_s || lobe.peak < cfg.top_brake_min.min(cfg.up_min) {
            return false;
        }
        if let Some(prev) = self.history.last_mut() {
            let gap = lobe.t0 - prev.t1;
            if gap > cfg.max_gap_s || (prev.sign == lobe.sign && gap > cfg.split_gap_s) {
                self.history.clear();
            } else if prev.sign == lobe.sign {
                // A lobe split by a dip too small to be a lobe of its own.
                prev.t1 = lobe.t1;
                if lobe.peak > prev.peak {
                    prev.peak = lobe.peak;
                    prev.peak_t = lobe.peak_t;
                }
                return false;
            }
        }
        self.history.push(lobe);
        if self.history.len() > 3 {
            self.history.remove(0);
        }
        if self.history.len() == 3 && self.history[0].sign == -1 && self.history[1].sign == 1 {
            let [first, bottom, last] = [self.history[0], self.history[1], self.history[2]];
            self.history = vec![last];
            if self.is_squat(&first, &bottom, &last) {
                self.count += 1;
                self.last_interval = if self.last_count_t.is_finite() {
                    (last.peak_t - self.last_count_t).min(self.cfg.max_cycle_s)
                } else {
                    0.0
                };
                self.last_count_t = last.peak_t;
                return true;
            }
        }
        false
    }

    fn is_squat(&self, first: &Lobe, bottom: &Lobe, last: &Lobe) -> bool {
        let cfg = &self.cfg;
        let cycle = last.peak_t - first.peak_t;
        if !(cfg.min_cycle_s..=cfg.max_cycle_s).contains(&cycle)
            || first.peak < cfg.down_min
            || bottom.peak < cfg.up_min
            || last.peak < cfg.top_brake_min
            || bottom.peak < cfg.bottom_ratio * first.peak.max(last.peak)
            || first.peak.min(last.peak) < cfg.edge_ratio * bottom.peak
            || last.peak_t - self.last_count_t
                < cfg
                    .min_rep_gap_s
                    .max(cfg.min_gap_ratio * self.last_interval)
        {
            return false;
        }
        // Velocity is zero where the movement starts: at the start of the first lobe if the
        // phone was still before it, else (the lobe is the end of the previous repetition) at
        // its peak.
        let t_start = if first.isolated {
            first.t0
        } else {
            first.peak_t
        };
        let samples: Vec<&Sample> = self
            .buffer
            .iter()
            .filter(|s| s.t >= t_start && s.t <= last.t1)
            .collect();
        let Some(window) = until_standstill(&samples, last.peak_t) else {
            return false;
        };
        let start_dir = window[0].dir;
        let net_turn = dot(window[window.len() - 1].dir, start_dir)
            .clamp(-1.0, 1.0)
            .acos();
        if net_turn.to_degrees() > cfg.max_net_turn_deg {
            return false;
        }
        let max_turn = window
            .iter()
            .map(|s| dot(s.dir, start_dir).clamp(-1.0, 1.0).acos())
            .fold(0.0, f64::max);
        let depth = depth_of(window);
        if max_turn.to_degrees() > cfg.max_rotation_deg {
            return false;
        }
        let strong = first.peak.min(bottom.peak).min(last.peak) >= cfg.strong_peak;
        depth <= cfg.max_depth_m && (strong || depth >= cfg.min_depth_m)
    }
}

/// The part of `samples` up to where the velocity (integrated from the first sample) is closest
/// to zero after `from_t`: the top of the movement, which is at the peak of the closing lobe
/// when the next repetition follows straight on and at its end when the phone comes to rest.
fn until_standstill<'a, 'b>(samples: &'b [&'a Sample], from_t: f64) -> Option<&'b [&'a Sample]> {
    let mut velocity = 0.0;
    let mut best: Option<(usize, f64)> = None;
    for i in 1..samples.len() {
        velocity += 0.5 * (samples[i].av + samples[i - 1].av) * (samples[i].t - samples[i - 1].t);
        if samples[i].t >= from_t && best.map_or(true, |(_, v)| velocity.abs() < v) {
            best = Some((i, velocity.abs()));
        }
    }
    let (end, _) = best?;
    (end >= 2).then(|| &samples[..=end])
}

/// Distance travelled between the first and the last sample, which are both at velocity zero:
/// integrate the acceleration to velocity, remove the leftover at the end linearly, integrate
/// again, and take the largest excursion.
fn depth_of(window: &[&Sample]) -> f64 {
    let (t_start, t_end) = (window[0].t, window[window.len() - 1].t);
    let mut velocity = vec![0.0; window.len()];
    for i in 1..window.len() {
        let dt = window[i].t - window[i - 1].t;
        velocity[i] = velocity[i - 1] + 0.5 * (window[i].av + window[i - 1].av) * dt;
    }
    let drift = velocity[window.len() - 1];
    let span = (t_end - t_start).max(1e-6);
    let corrected = |i: usize| velocity[i] - drift * (window[i].t - t_start) / span;
    let (mut position, mut deepest) = (0.0_f64, 0.0_f64);
    for i in 1..window.len() {
        let dt = window[i].t - window[i - 1].t;
        position += 0.5 * (corrected(i) + corrected(i - 1)) * dt;
        deepest = deepest.max(position.abs());
    }
    deepest
}
