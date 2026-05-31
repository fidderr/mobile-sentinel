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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Active,
    Completed,
}

/// Guardian behavior configuration for a single job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    /// Polling interval in milliseconds. Default: 500.
    pub poll_interval_ms: u64,

    /// Delay before starting MAIN after detecting it's dead. Default: 0.
    pub start_main_delay_ms: u64,

    /// Delay before sending the heads-up broadcast when MAIN is alive. Default: 200.
    pub heads_up_delay_ms: u64,

    /// Whether to auto-remove the job file on completion. Default: true.
    pub auto_remove_on_complete: bool,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 500,
            start_main_delay_ms: 0,
            heads_up_delay_ms: 200,
            auto_remove_on_complete: true,
        }
    }
}

/// A sentinel job file. One JSON file per job in `<files>/sentinel/jobs/<id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job identifier (consumer chooses).
    pub id: String,

    /// Job status — sentinel only acts on "active" jobs.
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

/// Get all active jobs (status == "active").
pub fn get_active_jobs() -> Result<Vec<Job>, JobGuardianError> {
    let dir = jobs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut jobs = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| JobGuardianError::Io(format!("read_dir {}: {}", dir.display(), e)))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<Job>(&contents) {
                Ok(job) if job.status == JobStatus::Active => {
                    jobs.push(job);
                }
                _ => {}
            },
            Err(_) => continue,
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
    let job: Job =
        serde_json::from_str(&contents).map_err(|e| JobGuardianError::Parse(e.to_string()))?;
    Ok(Some(job))
}

/// Remove a job file entirely.
pub fn remove_job(id: &str) -> Result<(), JobGuardianError> {
    let path = job_path(id);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| JobGuardianError::Io(format!("remove {}: {}", path.display(), e)))?;
    }
    Ok(())
}

/// Activate a pending job (set status to "active"). No-op if already active.
pub fn activate_job(id: &str) -> Result<(), JobGuardianError> {
    let path = job_path(id);
    if !path.exists() {
        return Err(JobGuardianError::NotFound(id.to_owned()));
    }
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| JobGuardianError::Io(format!("read {}: {}", path.display(), e)))?;
    let mut job: Job =
        serde_json::from_str(&contents).map_err(|e| JobGuardianError::Parse(e.to_string()))?;
    if job.status == JobStatus::Pending {
        job.status = JobStatus::Active;
        write_job(&job)?;
    }
    Ok(())
}

/// Deactivate a job — set its status back to "pending" without removing
/// the file. Use this when the job no longer needs MAIN alive *right now*
/// but will be re-activated by a future trigger (e.g. an alarm that was
/// snoozed: the firing session is over, but the snooze re-fire will flip
/// the same job pending → active again).
///
/// The distinction from [`complete_job`]:
/// - `complete_job` means "this job is done" → removes the file (or marks
///   Completed). The guardian will never act on it again.
/// - `deactivate_job` means "pause until the next trigger" → keeps the
///   file as Pending so [`activate_job`] can revive it.
///
/// Once the file is Pending, the guardian's poll loop sees no active jobs
/// and stops resurrecting MAIN. No-op if the job is already Pending;
/// errors only on I/O / parse failure (a missing file is treated as
/// already-gone and returns `Ok`).
pub fn deactivate_job(id: &str) -> Result<(), JobGuardianError> {
    let path = job_path(id);
    if !path.exists() {
        // Nothing to deactivate — treat as already inactive.
        return Ok(());
    }
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| JobGuardianError::Io(format!("read {}: {}", path.display(), e)))?;
    let mut job: Job =
        serde_json::from_str(&contents).map_err(|e| JobGuardianError::Parse(e.to_string()))?;
    if job.status != JobStatus::Pending {
        job.status = JobStatus::Pending;
        write_job(&job)?;
    }
    Ok(())
}

// --- Internal helpers ---

fn job_path(id: &str) -> PathBuf {
    jobs_dir().join(format!("{}.json", id))
}

fn write_job(job: &Job) -> Result<(), JobGuardianError> {
    let dir = jobs_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| JobGuardianError::Io(format!("create_dir {}: {}", dir.display(), e)))?;
    }
    let path = job_path(&job.id);
    let json =
        serde_json::to_string_pretty(job).map_err(|e| JobGuardianError::Parse(e.to_string()))?;
    std::fs::write(&path, json)
        .map_err(|e| JobGuardianError::Io(format!("write {}: {}", path.display(), e)))?;
    Ok(())
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
}
