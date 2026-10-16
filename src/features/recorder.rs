//! Recorder capability — record audio from the microphone (voice notes).
//!
//! Gated behind the `recorder` Cargo feature. One recording at a time:
//! [`start`], poll [`level`] for a meter, then [`stop`] (keep) or
//! [`cancel`] (discard).
//!
//! | Platform | Backend | Output |
//! |---|---|---|
//! | Android | `MediaRecorder` (`com.mobilesentinel.SentinelRecorderPrimitives`) | AAC in `.m4a` |
//! | Windows | `cpal` (WASAPI) + `hound` | 16-bit mono `.wav` |
//!
//! On Android the app needs `RECORD_AUDIO` at runtime: check
//! [`has_permission`], call [`request_permission`] (shows the system dialog,
//! returns immediately) and re-check before calling [`start`]. The
//! permission itself is declared in the manifest automatically when the
//! feature is enabled.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["recorder"] }
//! ```

use std::path::Path;

use crate::error::{ErrorCode, SentinelError};

/// A finished recording.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recording {
    /// Absolute path of the audio file.
    pub path: String,
    /// Length in milliseconds.
    pub duration_ms: u64,
}

fn failed(message: &str) -> SentinelError {
    SentinelError::RuntimeError {
        code: ErrorCode::Internal,
        message: message.to_string(),
    }
}

/// `true` if recording is allowed right now (always `true` off Android).
pub fn has_permission() -> bool {
    #[cfg(target_os = "android")]
    {
        android::has_permission()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

/// Ask for the microphone permission (Android shows the system dialog).
/// Returns immediately; re-check [`has_permission`] afterwards.
pub fn request_permission() {
    #[cfg(target_os = "android")]
    android::request_permission();
}

/// Start recording into a new file under `dir`. Fails if a recording is
/// already running, the permission is missing, or there is no microphone.
pub fn start(dir: &Path) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        match android::start(&dir.to_string_lossy()) {
            0 => Ok(()),
            1 => Err(SentinelError::RuntimeError {
                code: ErrorCode::InvalidArgument,
                message: "RECORD_AUDIO permission not granted".into(),
            }),
            3 => Err(SentinelError::RuntimeError {
                code: ErrorCode::ResourceBusy,
                message: "already recording".into(),
            }),
            _ => Err(failed("could not start the recorder")),
        }
    }
    #[cfg(windows)]
    {
        windows::start(dir)
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        let _ = dir;
        Err(SentinelError::unavailable("recorder::start"))
    }
}

/// Current input level, 0.0–1.0, for a live meter. 0 when not recording.
pub fn level() -> f32 {
    #[cfg(target_os = "android")]
    {
        android::level()
    }
    #[cfg(windows)]
    {
        windows::level()
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        0.0
    }
}

/// `true` while a recording is running.
pub fn is_recording() -> bool {
    #[cfg(target_os = "android")]
    {
        android::is_recording()
    }
    #[cfg(windows)]
    {
        windows::is_recording()
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        false
    }
}

/// Stop and keep the recording.
pub fn stop() -> Result<Recording, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let json = android::stop();
        serde_json::from_str(&json).map_err(|_| failed("recording too short or failed"))
    }
    #[cfg(windows)]
    {
        windows::stop()
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        Err(SentinelError::unavailable("recorder::stop"))
    }
}

/// Stop and delete the recording.
pub fn cancel() {
    #[cfg(target_os = "android")]
    android::cancel();
    #[cfg(windows)]
    windows::cancel();
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};

    const RECORDER: &str = "com/mobilesentinel/SentinelRecorderPrimitives";

    pub(super) fn has_permission() -> bool {
        with_jni_class(RECORDER, false, |env, class| {
            env.call_static_method(class, "hasPermission", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn request_permission() {
        with_jni_class(RECORDER, (), |env, class| {
            env.call_static_method(class, "requestPermission", "()V", &[])
                .ok()
                .map(|_| ())
        })
    }

    pub(super) fn start(dir: &str) -> i32 {
        with_jni_class(RECORDER, 2, |env, class| {
            let s = jni_str(env, dir)?;
            env.call_static_method(
                class,
                "start",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.i().ok())
        })
    }

    pub(super) fn level() -> f32 {
        with_jni_class(RECORDER, 0.0, |env, class| {
            env.call_static_method(class, "level", "()F", &[])
                .ok()
                .and_then(|v| v.f().ok())
        })
    }

    pub(super) fn is_recording() -> bool {
        with_jni_class(RECORDER, false, |env, class| {
            env.call_static_method(class, "isRecording", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn stop() -> String {
        with_jni_class(RECORDER, String::new(), |env, class| {
            let obj = env
                .call_static_method(class, "stop", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            let jstr: JString = obj.into();
            env.get_string(&jstr).ok().map(Into::into)
        })
    }

    pub(super) fn cancel() {
        with_jni_class(RECORDER, (), |env, class| {
            env.call_static_method(class, "cancel", "()V", &[])
                .ok()
                .map(|_| ())
        })
    }
}

#[cfg(windows)]
mod windows {
    //! cpal capture on a dedicated thread (cpal streams are not `Send`),
    //! samples down-mixed to mono i16 and written as WAV on stop.

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::{failed, Recording};
    use crate::error::{ErrorCode, SentinelError};

    struct Session {
        stop: mpsc::Sender<()>,
        thread: Option<std::thread::JoinHandle<()>>,
        samples: Arc<Mutex<Vec<i16>>>,
        rate: u32,
        started: Instant,
        path: PathBuf,
    }

    static SESSION: Mutex<Option<Session>> = Mutex::new(None);
    static LEVEL: AtomicU32 = AtomicU32::new(0);

    fn lock() -> std::sync::MutexGuard<'static, Option<Session>> {
        SESSION.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(super) fn start(dir: &Path) -> Result<(), SentinelError> {
        let mut guard = lock();
        if guard.is_some() {
            return Err(SentinelError::RuntimeError {
                code: ErrorCode::ResourceBusy,
                message: "already recording".into(),
            });
        }
        std::fs::create_dir_all(dir).map_err(|e| failed(&e.to_string()))?;
        let path = dir.join(format!("{}.wav", uuid::Uuid::new_v4()));
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();

        let buf = samples.clone();
        let thread = std::thread::spawn(move || {
            let run = || -> Result<(cpal::Stream, u32), String> {
                let device = cpal::default_host()
                    .default_input_device()
                    .ok_or("no microphone")?;
                let config = device.default_input_config().map_err(|e| e.to_string())?;
                let rate = config.sample_rate().0;
                let channels = config.channels() as usize;
                let err_fn = |e| log::warn!("[recorder] stream error: {e}");
                let stream = match config.sample_format() {
                    cpal::SampleFormat::F32 => device.build_input_stream(
                        &config.into(),
                        move |data: &[f32], _| push(&buf, data, channels, |s| s),
                        err_fn,
                        None,
                    ),
                    cpal::SampleFormat::I16 => device.build_input_stream(
                        &config.into(),
                        move |data: &[i16], _| {
                            push(&buf, data, channels, |s| s as f32 / i16::MAX as f32)
                        },
                        err_fn,
                        None,
                    ),
                    other => return Err(format!("unsupported sample format {other:?}")),
                }
                .map_err(|e| e.to_string())?;
                stream.play().map_err(|e| e.to_string())?;
                Ok((stream, rate))
            };
            match run() {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
            LEVEL.store(0, Ordering::Relaxed);
        });

        let rate = ready_rx
            .recv()
            .map_err(|_| failed("recorder thread died"))?
            .map_err(|e| failed(&e))?;
        *guard = Some(Session {
            stop: stop_tx,
            thread: Some(thread),
            samples,
            rate,
            started: Instant::now(),
            path,
        });
        Ok(())
    }

    /// Down-mix interleaved frames to mono i16 and update the level meter.
    fn push<T: Copy>(
        buf: &Arc<Mutex<Vec<i16>>>,
        data: &[T],
        channels: usize,
        to_f32: impl Fn(T) -> f32,
    ) {
        let mut peak = 0f32;
        let mut out = buf.lock().unwrap_or_else(|p| p.into_inner());
        for frame in data.chunks(channels.max(1)) {
            let v = frame.iter().map(|s| to_f32(*s)).sum::<f32>() / frame.len() as f32;
            peak = peak.max(v.abs());
            out.push((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
        LEVEL.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
    }

    pub(super) fn level() -> f32 {
        f32::from_bits(LEVEL.load(Ordering::Relaxed))
    }

    pub(super) fn is_recording() -> bool {
        lock().is_some()
    }

    fn finish() -> Option<Session> {
        let mut session = lock().take()?;
        let _ = session.stop.send(());
        if let Some(t) = session.thread.take() {
            let _ = t.join();
        }
        Some(session)
    }

    pub(super) fn stop() -> Result<Recording, SentinelError> {
        let session = finish().ok_or_else(|| failed("not recording"))?;
        let duration_ms = session.started.elapsed().as_millis() as u64;
        let samples = session.samples.lock().unwrap_or_else(|p| p.into_inner());
        if samples.is_empty() {
            return Err(failed("recording too short"));
        }
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: session.rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let write = || -> Result<(), hound::Error> {
            let mut w = hound::WavWriter::create(&session.path, spec)?;
            for s in samples.iter() {
                w.write_sample(*s)?;
            }
            w.finalize()
        };
        write().map_err(|e| failed(&e.to_string()))?;
        Ok(Recording {
            path: session.path.to_string_lossy().into_owned(),
            duration_ms,
        })
    }

    pub(super) fn cancel() {
        if let Some(s) = finish() {
            let _ = std::fs::remove_file(&s.path);
        }
    }
}
