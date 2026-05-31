package com.mobilesentinel

import android.util.Log

/**
 * JNI bridge between Android system callbacks and the Rust mobile-sentinel layer.
 * This object holds native method declarations that call into Rust.
 * Android system components (receivers, services, sensor helpers) call the
 * `onXxx` wrappers, which forward to the matching `nativeOnXxx` JNI export in
 * `src/platform/android/callbacks.rs`.
 *
 * Every declared native method has a matching Rust export. The set is
 * intentionally small — the universal SDK delivers most capabilities through
 * blocking primitive calls (scan, pick, biometric) and the on-disk job-file
 * contract, not through async JNI callbacks.
 *
 * Apps using mobile-sentinel never interact with this directly — it's internal
 * plumbing between Android OS events and the Rust event system.
 */
object SentinelBridge {

    private const val TAG = "MobileSentinel.Bridge"

    /**
 * Name of the native shared library to load. Defaults to `"main"` which
 * is the convention used by Dioxus and most Rust mobile frameworks.
 * Consumers using a different library name (e.g. via cargo-ndk) can set
 * this before the first JNI call:
 * ```kotlin
 * SentinelBridge.nativeLibraryName = "myapp"
 * ```
 */
    @JvmStatic
    var nativeLibraryName: String = "main"

    init {
        try {
 // Load the native library that contains our JNI functions.
 // The library name is configurable — defaults to "main" (libmain.so).
            System.loadLibrary(nativeLibraryName)
            Log.i(TAG, "SentinelBridge: native library '$nativeLibraryName' loaded successfully")
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "SentinelBridge: failed to load native library '$nativeLibraryName'", e)
        } catch (e: Exception) {
            Log.e(TAG, "SentinelBridge: init failed", e)
        }
    }

    /**
 * Called on boot completed. Dispatches to Rust's boot_completed callback.
 */
    @JvmStatic
    fun onBootCompleted() {
        try {
            nativeOnBootCompleted()
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "nativeOnBootCompleted not linked yet")
        }
    }

 // --- Sensor callbacks ---

    @JvmStatic
    fun onAccelerometerData(x: Double, y: Double, z: Double) {
        try {
            nativeOnAccelerometerData(x, y, z)
        } catch (e: UnsatisfiedLinkError) {
 // Expected during init — sensors may fire before native lib loads
        }
    }

    @JvmStatic
    fun onStepCount(steps: Int) {
        try {
            nativeOnStepCount(steps)
        } catch (e: UnsatisfiedLinkError) {
 // Expected during init
        }
    }

    /**
     * Called when the job guardian sends a JOB_HEADS_UP broadcast while
     * MAIN is alive. Forwards to Rust which dispatches the job's action.
     */
    @JvmStatic
    fun onJobHeadsUp(jobId: String) {
        try {
            nativeOnJobHeadsUp(jobId)
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "nativeOnJobHeadsUp not linked yet")
        }
    }

 // --- Private native method declarations (each has a Rust export) ---

    @JvmStatic
    private external fun nativeOnBootCompleted()

    @JvmStatic
    private external fun nativeOnAccelerometerData(x: Double, y: Double, z: Double)

    @JvmStatic
    private external fun nativeOnStepCount(steps: Int)

    @JvmStatic
    private external fun nativeOnJobHeadsUp(jobId: String)
}
