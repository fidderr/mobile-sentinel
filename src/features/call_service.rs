//! Call service capability: what a live call needs from Android. Gated behind `call_service`.
//!
//! A foreground service keeps the microphone and camera going while the app is not in front, which
//! Android otherwise cuts off. It can also show the screen: Android asks the user each time, and
//! encoded frames come straight here, so they keep coming while the app is behind another one.
//! Nothing here does anything on other platforms. The JNI calls into
//! `com.mobilesentinel.SentinelCallPrimitives` live here.

/// How a frame of the screen is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenCodec {
    H264,
    Vp8,
}

/// One encoded frame of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFrame {
    pub data: Vec<u8>,
    /// When it was captured, in microseconds.
    pub time_us: i64,
    /// A whole picture, that can be read without the frames before it.
    pub key: bool,
    pub codec: ScreenCodec,
}

/// Starts or updates the service, with this in its notification. Android only allows this while the
/// app is in front, and with the permissions for what it says it uses. `camera`: the camera may be
/// used too.
pub fn start(title: &str, text: &str, camera: bool) -> bool {
    #[cfg(target_os = "android")]
    {
        android::start(title, text, camera)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (title, text, camera);
        false
    }
}

/// Stops the service, and showing the screen with it.
pub fn stop() {
    #[cfg(target_os = "android")]
    android::stop();
}

/// Asks Android whether the screen may be shown. `on_state` hears the answer and every change
/// after that, `Some((width, height))` while it is shown and `None` when it is not (also when the
/// user said no, the status bar chip was tapped, or the screen was locked). `on_frame` gets the
/// frames. Both run on a system thread: pass them on. `stop_label` is the button in the notification.
pub fn show_screen<S, F>(stop_label: &str, on_state: S, on_frame: F) -> bool
where
    S: Fn(Option<(u32, u32)>) + Send + Sync + 'static,
    F: Fn(ScreenFrame) + Send + Sync + 'static,
{
    #[cfg(target_os = "android")]
    {
        *android::ON_STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(on_state));
        *android::ON_FRAME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(on_frame));
        android::ask_for_screen(stop_label)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (stop_label, on_state, on_frame);
        false
    }
}

/// Stops showing the screen. The call goes on.
pub fn stop_screen() {
    #[cfg(target_os = "android")]
    android::stop_screen();
}

/// The next frame is a whole picture.
pub fn want_whole_picture() {
    #[cfg(target_os = "android")]
    android::want_whole_picture();
}

/// A call's sound from the speaker, or from what the system picks: the earpiece or a headset.
pub fn set_speaker(on: bool) -> bool {
    #[cfg(target_os = "android")]
    {
        android::set_speaker(on)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = on;
        false
    }
}

/// The call is over: sound goes back to how it was.
pub fn end_audio() {
    #[cfg(target_os = "android")]
    android::end_audio();
}

#[cfg(target_os = "android")]
mod android {
    use super::{ScreenCodec, ScreenFrame};
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::JValue;
    use std::sync::Mutex;

    const CALL: &str = "com/mobilesentinel/SentinelCallPrimitives";

    type StateCallback = Box<dyn Fn(Option<(u32, u32)>) + Send + Sync>;
    type FrameCallback = Box<dyn Fn(ScreenFrame) + Send + Sync>;
    pub(super) static ON_STATE: Mutex<Option<StateCallback>> = Mutex::new(None);
    pub(super) static ON_FRAME: Mutex<Option<FrameCallback>> = Mutex::new(None);

    #[no_mangle]
    pub extern "system" fn Java_com_mobilesentinel_SentinelCallPrimitives_nativeOnScreen(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
        on: jni::sys::jboolean,
        width: jni::sys::jint,
        height: jni::sys::jint,
    ) {
        if let Ok(callback) = ON_STATE.lock() {
            if let Some(callback) = callback.as_ref() {
                callback((on != 0).then_some((width.max(0) as u32, height.max(0) as u32)));
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_mobilesentinel_SentinelCallPrimitives_nativeOnScreenFrame(
        env: jni::JNIEnv,
        _class: jni::objects::JClass,
        data: jni::objects::JByteArray,
        time_us: jni::sys::jlong,
        key: jni::sys::jboolean,
        codec: jni::sys::jint,
    ) {
        let Ok(data) = env.convert_byte_array(&data) else {
            return;
        };
        if let Ok(callback) = ON_FRAME.lock() {
            if let Some(callback) = callback.as_ref() {
                callback(ScreenFrame {
                    data,
                    time_us,
                    key: key != 0,
                    codec: if codec == 2 {
                        ScreenCodec::H264
                    } else {
                        ScreenCodec::Vp8
                    },
                });
            }
        }
    }

    pub(super) fn start(title: &str, text: &str, camera: bool) -> bool {
        with_jni_class(CALL, false, |env, class| {
            let title = jni_str(env, title)?;
            let text = jni_str(env, text)?;
            env.call_static_method(
                class,
                "startCall",
                "(Ljava/lang/String;Ljava/lang/String;Z)Z",
                &[
                    JValue::Object(&title.into()),
                    JValue::Object(&text.into()),
                    JValue::Bool(u8::from(camera)),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    fn call_void(name: &'static str) {
        with_jni_class(CALL, (), |env, class| {
            env.call_static_method(class, name, "()V", &[])
                .ok()
                .map(|_| ())
        });
    }

    pub(super) fn stop() {
        call_void("stopCall");
    }

    pub(super) fn stop_screen() {
        call_void("stopScreen");
    }

    pub(super) fn want_whole_picture() {
        call_void("wantWholePicture");
    }

    pub(super) fn end_audio() {
        call_void("endAudio");
    }

    pub(super) fn ask_for_screen(stop_label: &str) -> bool {
        with_jni_class(CALL, false, |env, class| {
            let label = jni_str(env, stop_label)?;
            env.call_static_method(
                class,
                "askForScreen",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&label.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn set_speaker(on: bool) -> bool {
        with_jni_class(CALL, false, |env, class| {
            env.call_static_method(class, "setSpeaker", "(Z)Z", &[JValue::Bool(u8::from(on))])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
