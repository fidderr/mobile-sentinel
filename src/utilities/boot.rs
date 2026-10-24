//! When the device last booted.
//!
//! Android clears every scheduled alarm and kills every process on reboot, so
//! anything persisted as "ringing" from before the current boot is stale.

/// Wall-clock time (unix ms) the device booted, derived from
/// `CLOCK_BOOTTIME` (which keeps counting through deep sleep). `None` where
/// it cannot be determined (host builds), in which case nothing is treated
/// as stale.
pub fn boot_unix_ms() -> Option<i64> {
    #[cfg(target_os = "android")]
    {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid, writable timespec for the duration of the call.
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
        if rc != 0 {
            return None;
        }
        let since_boot_ms = i64::from(ts.tv_sec) * 1000 + i64::from(ts.tv_nsec) / 1_000_000;
        Some(chrono::Utc::now().timestamp_millis() - since_boot_ms)
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}
