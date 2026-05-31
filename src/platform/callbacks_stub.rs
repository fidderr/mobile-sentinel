//! No-op callback stubs for non-Android platforms.
//! These allow app code to unconditionally register callbacks without
//! cfg-gating every call site. On non-Android they simply do nothing.

/// Register a callback for device boot completed (no-op on non-Android).
pub fn on_boot_completed<F>(_callback: F)
where
    F: Fn() + Send + Sync + 'static,
{
}

/// Register a callback for job-guardian heads-up broadcasts (no-op on non-Android).
pub fn on_job_heads_up<F>(_callback: F)
where
    F: Fn(String) + Send + Sync + 'static,
{
}
