//! Sentinel Job Guardian — generic polling-based job persistence for the
//! `:sentinel` process.
//!
//! The job guardian is completely generic. It knows nothing about alarms,
//! audio, or kiosk. It only knows:
//! - "I have jobs that need MAIN alive"
//! - "Is the job done? No → keep polling. Yes → stop polling"
//!
//! Consumers (alarm apps, timer apps, reminder apps) register jobs with
//! an opaque `payload` field that only they interpret in MAIN.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Job status — sentinel only cares about "active" vs "completed".
///
/// `Active` is never stored in the job file: a job is active while its active
/// marker exists (see [`ACTIVE_SUFFIX`]). The marker is created by whoever
/// noticed the job must run (the `:sentinel` alarm receiver) and removed by
/// Rust, while Rust alone writes the job file, so neither can overwrite the
/// other's change: saving a job at the moment it fires cannot lose the fire.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    #[default]
    Pending,
    Active,
    Completed,
}

/// Suffix of a job's active marker, next to its job file:
/// `<files>/sentinel/jobs/<id>.active`. Its modification time is when the job
/// became active. Mirrored by `SentinelActiveJobs` in Kotlin.
pub const ACTIVE_SUFFIX: &str = ".active";

/// Suffix of the marker the ring service leaves when a ring reached its limit,
/// next to the job file: `<files>/sentinel/jobs/<id>.limit`. Created by Kotlin,
/// which cannot end the alarm itself, and removed here with the active marker.
/// Mirrored by `SentinelActiveJobs` in Kotlin.
pub const LIMIT_SUFFIX: &str = ".limit";

/// Suffix of the marker the backup screen leaves when its challenge was solved:
/// `<files>/sentinel/jobs/<id>.dismiss`. Same arrangement as [`LIMIT_SUFFIX`].
pub const DISMISS_SUFFIX: &str = ".dismiss";

/// How a ring ended that the platform ended by itself, with Rust not around to
/// decide; see [`ring_end`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingEnd {
    /// It reached its limit with nobody responding.
    Limit,
    /// The user solved the challenge on the backup screen.
    BackupScreen,
}

/// Guardian behavior configuration for a single job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobConfig {
    /// Polling interval in milliseconds. Default: 500.
    pub poll_interval_ms: u64,

    /// Delay before starting MAIN after detecting it's dead. Default: 0.
    pub start_main_delay_ms: u64,

    /// Delay before sending the heads-up broadcast when MAIN is alive. Default: 200.
    pub heads_up_delay_ms: u64,

    /// Whether to auto-remove the job file on completion. Default: true.
    pub auto_remove_on_complete: bool,

    /// How long MAIN's UI may go without a [`ui_heartbeat`] while the app's
    /// main activity is on screen and this job is active, before the
    /// guardian kills MAIN so it is relaunched. `0` (default) turns the
    /// check off. Missing in job files written by older versions.
    #[serde(default)]
    pub ui_liveness_timeout_ms: u64,

    /// After the guardian kills MAIN for a frozen UI, the allowed silence
    /// doubles with every further kill, up to this many times
    /// `ui_liveness_timeout_ms` (so a UI that is merely slow to start is not
    /// killed in a loop). Default: 4.
    #[serde(default = "default_ui_liveness_backoff_cap")]
    pub ui_liveness_backoff_cap: u32,

    /// Minimum gap between two launches of MAIN. A launch delivers a new
    /// intent that pauses and resumes the activity, so launching again while
    /// a cold start is still under way cancels the user's touches.
    /// Default: 3000.
    #[serde(default = "default_min_launch_interval_ms")]
    pub min_launch_interval_ms: u64,

    /// After this many times the UI failed (frozen, or died before it ever came up)
    /// the platform's backup screen takes over, if the firing plan has one. `0`
    /// (default) means never.
    #[serde(default)]
    pub ui_failures_before_backup: u32,
}

fn default_ui_liveness_backoff_cap() -> u32 {
    4
}

fn default_min_launch_interval_ms() -> u64 {
    3_000
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 500,
            start_main_delay_ms: 0,
            heads_up_delay_ms: 200,
            auto_remove_on_complete: true,
            ui_liveness_timeout_ms: 0,
            ui_liveness_backoff_cap: default_ui_liveness_backoff_cap(),
            ui_failures_before_backup: 0,
            min_launch_interval_ms: default_min_launch_interval_ms(),
        }
    }
}

/// File (under the jobs directory) whose modification time is the last UI
/// heartbeat. Mirrored by `SentinelJobGuardian`.
pub const UI_HEARTBEAT_FILE: &str = "ui_heartbeat";

/// Tell the `:sentinel` guardian the UI is alive. Call it periodically from
/// the UI, and only when something that needs the UI to work has just
/// worked (for a WebView app, a round trip through the page) — a heartbeat
/// from a loop that can outlive a frozen UI defeats the check. Cheap: one
/// small file write.
pub fn ui_heartbeat() {
    let dir = jobs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    // Content is irrelevant, the guardian reads the modification time; a
    // changing value just guarantees the mtime moves on coarse filesystems.
    let stamp = chrono::Utc::now().timestamp_millis().to_string();
    let _ = std::fs::write(dir.join(UI_HEARTBEAT_FILE), stamp);
}

/// A sentinel job file. One JSON file per job in `<files>/sentinel/jobs/<id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job identifier (consumer chooses).
    pub id: String,

    /// Job status — sentinel only acts on "active" jobs. Derived from the
    /// active marker when a job is read.
    #[serde(default)]
    pub status: JobStatus,

    /// Opaque JSON payload — sentinel never reads this, consumer uses it in MAIN.
    pub payload: serde_json::Value,

    /// Guardian behavior settings.
    pub config: JobConfig,
}

/// Returns the directory where job files are stored.
pub fn jobs_dir() -> PathBuf {
    crate::utilities::app_files_dir()
        .join("sentinel")
        .join("jobs")
}

/// Register a new job. Writes the job JSON file to disk.
///
/// - `id`: unique job identifier (consumer chooses)
/// - `payload`: opaque JSON — sentinel never reads this
/// - `config`: guardian behavior settings (use `JobConfig::default()` for sensible defaults)
pub fn register_job(
    id: impl Into<String>,
    payload: serde_json::Value,
    config: JobConfig,
) -> Result<Job, JobGuardianError> {
    let id = id.into();
    let job = Job {
        id: id.clone(),
        status: JobStatus::Pending,
        payload,
        config,
    };
    write_job(&job)?;
    Ok(job)
}

/// Mark a job as completed. If `auto_remove_on_complete` is true in the
/// job's config, the file is deleted instead of updated.
pub fn complete_job(id: &str) -> Result<(), JobGuardianError> {
    remove_marker(id)?;
    let path = job_path(id);
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| JobGuardianError::Io(format!("read {}: {}", path.display(), e)))?;
    let mut job: Job =
        serde_json::from_str(&contents).map_err(|e| JobGuardianError::Parse(e.to_string()))?;

    if job.config.auto_remove_on_complete {
        std::fs::remove_file(&path)
            .map_err(|e| JobGuardianError::Io(format!("remove {}: {}", path.display(), e)))?;
    } else {
        job.status = JobStatus::Completed;
        write_job(&job)?;
    }
    Ok(())
}

/// Get all active jobs (those with an active marker and a readable job file).
pub fn get_active_jobs() -> Result<Vec<Job>, JobGuardianError> {
    let dir = jobs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut jobs = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| JobGuardianError::Io(format!("read_dir {}: {}", dir.display(), e)))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_suffix(ACTIVE_SUFFIX)) else {
            continue;
        };
        if let Ok(Some(job)) = get_job(id) {
            if job.status == JobStatus::Active {
                jobs.push(job);
            }
        }
    }
    Ok(jobs)
}

/// Get a specific job by id.
pub fn get_job(id: &str) -> Result<Option<Job>, JobGuardianError> {
    let path = job_path(id);
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| JobGuardianError::Io(format!("read {}: {}", path.display(), e)))?;
    let mut job: Job =
        serde_json::from_str(&contents).map_err(|e| JobGuardianError::Parse(e.to_string()))?;
    if job.status == JobStatus::Active {
        // Written by a version that stored the status in the job file.
        job.status = JobStatus::Pending;
    }
    if job.status == JobStatus::Pending && marker_path(id).exists() {
        job.status = JobStatus::Active;
    }
    Ok(Some(job))
}

/// Remove a job file entirely.
pub fn remove_job(id: &str) -> Result<(), JobGuardianError> {
    remove_marker(id)?;
    let path = job_path(id);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| JobGuardianError::Io(format!("remove {}: {}", path.display(), e)))?;
    }
    Ok(())
}

/// Activate a job by creating its active marker. No-op if already active, so
/// the marker keeps the time the job first became active.
pub fn activate_job(id: &str) -> Result<(), JobGuardianError> {
    if !job_path(id).exists() {
        return Err(JobGuardianError::NotFound(id.to_owned()));
    }
    let marker = marker_path(id);
    if marker.exists() {
        return Ok(());
    }
    let stamp = chrono::Utc::now().timestamp_millis().to_string();
    std::fs::write(&marker, stamp)
        .map_err(|e| JobGuardianError::Io(format!("write {}: {}", marker.display(), e)))
}

/// When the job became active (unix ms), if it is.
pub fn active_since_ms(id: &str) -> Option<i64> {
    let modified = std::fs::metadata(marker_path(id)).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

/// How this job's ring ended, if the platform ended it by itself and Rust has not
/// dealt with that yet. A solved challenge wins from a limit reached before it.
pub fn ring_end(id: &str) -> Option<RingEnd> {
    if end_marker_path(id, DISMISS_SUFFIX).exists() {
        Some(RingEnd::BackupScreen)
    } else if end_marker_path(id, LIMIT_SUFFIX).exists() {
        Some(RingEnd::Limit)
    } else {
        None
    }
}

/// Deactivate a job: remove its active marker and keep the job file. Use this
/// when the job no longer needs MAIN alive *right now* but will be re-activated
/// by a future trigger (e.g. an alarm that was snoozed: the firing session is
/// over, but the snooze re-fire activates the same job again).
///
/// The distinction from [`complete_job`]:
/// - `complete_job` means "this job is done" → removes the file (or marks
///   Completed). The guardian will never act on it again.
/// - `deactivate_job` means "pause until the next trigger" → keeps the file so
///   a later activation can revive it.
///
/// Once no marker is left, the guardian's poll loop sees no active jobs and
/// stops resurrecting MAIN. No-op when the job is not active.
pub fn deactivate_job(id: &str) -> Result<(), JobGuardianError> {
    remove_marker(id)
}

// --- Internal helpers ---

fn job_path(id: &str) -> PathBuf {
    jobs_dir().join(format!("{}.json", id))
}

fn marker_path(id: &str) -> PathBuf {
    jobs_dir().join(format!("{id}{ACTIVE_SUFFIX}"))
}

fn end_marker_path(id: &str, suffix: &str) -> PathBuf {
    jobs_dir().join(format!("{id}{suffix}"))
}

fn remove_marker(id: &str) -> Result<(), JobGuardianError> {
    let _ = std::fs::remove_file(end_marker_path(id, LIMIT_SUFFIX));
    let _ = std::fs::remove_file(end_marker_path(id, DISMISS_SUFFIX));
    let marker = marker_path(id);
    match std::fs::remove_file(&marker) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(JobGuardianError::Io(format!(
            "remove {}: {}",
            marker.display(),
            e
        ))),
    }
}

fn write_job(job: &Job) -> Result<(), JobGuardianError> {
    let path = job_path(&job.id);
    let json =
        serde_json::to_string_pretty(job).map_err(|e| JobGuardianError::Parse(e.to_string()))?;
    // Atomic replace: the `:sentinel` process polls these files, so it must
    // never see a half-written job.
    crate::utilities::atomic_write::write_atomic(&path, json.as_bytes())
        .map_err(|e| JobGuardianError::Io(format!("write {}: {}", path.display(), e)))
}

/// Errors from the job guardian API.
#[derive(Debug, thiserror::Error)]
pub enum JobGuardianError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("JSON parse error: {0}")]
    Parse(String),
    #[error("job not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Override the jobs dir for tests by using a temp directory.
    /// Since `jobs_dir()` uses `app_files_dir()` which is process-global,
    /// we test the internal helpers directly with explicit paths.

    #[test]
    fn job_config_defaults_are_sensible() {
        let cfg = JobConfig::default();
        assert_eq!(cfg.poll_interval_ms, 500);
        assert_eq!(cfg.start_main_delay_ms, 0);
        assert_eq!(cfg.heads_up_delay_ms, 200);
        assert!(cfg.auto_remove_on_complete);
        assert_eq!(cfg.ui_liveness_timeout_ms, 0, "liveness check is opt-in");
        assert_eq!(cfg.ui_liveness_backoff_cap, 4);
        assert_eq!(cfg.min_launch_interval_ms, 3_000);
    }

    #[test]
    fn job_files_written_before_the_liveness_field_still_load() {
        let old = r#"{"poll_interval_ms":100,"start_main_delay_ms":0,
            "heads_up_delay_ms":200,"auto_remove_on_complete":true}"#;
        let cfg: JobConfig = serde_json::from_str(old).unwrap();
        assert_eq!(cfg.poll_interval_ms, 100);
        assert_eq!(cfg.ui_liveness_timeout_ms, 0);
        assert_eq!(
            cfg.min_launch_interval_ms, 3_000,
            "a missing field takes its default, not zero"
        );
        assert_eq!(cfg.ui_liveness_backoff_cap, 4);
    }

    #[test]
    fn job_serialization_roundtrip() {
        let job = Job {
            id: "test-123".into(),
            status: JobStatus::Active,
            payload: serde_json::json!({"instance_id": "alarm-1"}),
            config: JobConfig::default(),
        };
        let json = serde_json::to_string(&job).unwrap();
        let parsed: Job = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-123");
        assert_eq!(parsed.status, JobStatus::Active);
        assert_eq!(parsed.payload["instance_id"], "alarm-1");
    }

    #[test]
    fn job_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&JobStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::Completed).unwrap(),
            "\"completed\""
        );
    }

    /// Full register → activate → deactivate → re-activate cycle, which is
    /// exactly the snooze lifecycle: a job that pauses to Pending between
    /// the firing session and the snooze re-fire, then revives. Uses a
    /// unique id so it doesn't collide with the process-global jobs dir.
    #[test]
    fn deactivate_resets_active_job_to_pending_and_can_reactivate() {
        let id = format!("test-deactivate-{}", std::process::id());
        // Clean any leftover from a prior aborted run.
        let _ = remove_job(&id);

        register_job(
            &id,
            serde_json::json!({"instance_id": id}),
            JobConfig::default(),
        )
        .unwrap();
        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Pending);

        // Fire → active.
        activate_job(&id).unwrap();
        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Active);

        // Snooze → deactivated back to pending (file MUST survive).
        deactivate_job(&id).unwrap();
        let job = get_job(&id).unwrap().expect("job file must still exist");
        assert_eq!(job.status, JobStatus::Pending);

        // Snooze re-fire → active again.
        activate_job(&id).unwrap();
        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Active);

        // deactivate is idempotent and tolerates a missing file.
        deactivate_job(&id).unwrap();
        remove_job(&id).unwrap();
        deactivate_job(&id).unwrap(); // no file → Ok

        // Cleanup.
        let _ = remove_job(&id);
    }

    /// The receiver marks a job active while the app may be saving it: writing
    /// the job file must not lose that.
    #[test]
    fn registering_a_job_again_keeps_it_active() {
        let id = format!("test-reregister-{}", std::process::id());
        let _ = remove_job(&id);
        let payload = serde_json::json!({"instance_id": id});
        register_job(&id, payload.clone(), JobConfig::default()).unwrap();
        activate_job(&id).unwrap();

        register_job(&id, payload, JobConfig::default()).unwrap();

        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Active);
        assert!(get_active_jobs().unwrap().iter().any(|j| j.id == id));
        assert!(active_since_ms(&id).is_some());
        remove_job(&id).unwrap();
        assert_eq!(active_since_ms(&id), None);
    }

    #[test]
    fn a_job_marked_active_by_kotlin_is_seen_as_active() {
        let id = format!("test-kotlin-marker-{}", std::process::id());
        let _ = remove_job(&id);
        register_job(&id, serde_json::json!({}), JobConfig::default()).unwrap();
        // All the receiver does: create the file.
        std::fs::write(jobs_dir().join(format!("{id}{ACTIVE_SUFFIX}")), "1").unwrap();

        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Active);
        deactivate_job(&id).unwrap();
        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Pending);
        remove_job(&id).unwrap();
    }

    #[test]
    fn job_files_that_stored_active_are_pending_without_a_marker() {
        let id = format!("test-old-active-{}", std::process::id());
        let _ = remove_job(&id);
        let mut job = register_job(&id, serde_json::json!({}), JobConfig::default()).unwrap();
        job.status = JobStatus::Active;
        write_job(&job).unwrap();

        assert_eq!(get_job(&id).unwrap().unwrap().status, JobStatus::Pending);
        remove_job(&id).unwrap();
    }
}
