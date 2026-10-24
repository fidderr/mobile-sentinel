//! When the device last booted.
//!
//! Android clears every scheduled alarm and kills every process on reboot, so
//! anything persisted as "ringing" from before the current boot is stale.
//!
//! This is the Rust half of one rule: *state written before the current boot is
//! stale*. Rust applies it to ring records in `AlarmKit::on_startup`. The other
//! half is `SentinelBoot` in `:sentinel-core` (Kotlin), applied to the kiosk
//! lock file and to active job files. Both exist because that Kotlin code must
//! decide before any Rust is running (the activity auto-engages the lock, the
//! guardian relaunches the app), and both compute the boot time the same way
//! (now minus time since boot). Change one, change the other.

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
        // `time_t` is 32 bits on 32-bit Android, so the widening is needed there
        // even though it is a no-op on 64-bit targets.
        #[allow(clippy::useless_conversion)]
        let since_boot_ms = i64::from(ts.tv_sec) * 1000 + i64::from(ts.tv_nsec) / 1_000_000;
        Some(chrono::Utc::now().timestamp_millis() - since_boot_ms)
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}
