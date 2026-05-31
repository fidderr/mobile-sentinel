package com.mobilesentinel

import android.content.Context
import android.content.pm.PackageManager
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.util.Log

/**
 * Universal SDK torch (flashlight) primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around Android's [CameraManager] — no orchestration logic, no state
 * machines, no Recipe-specific behavior. Exposes only steady-on control
 * (no strobe). Reached from Rust via `primitives_ext::torch_*`.
 *
 * Threading: every method is `@JvmStatic` and stateless apart from the
 * lazily-resolved [torchCameraId] cache. JNI callers are responsible for
 * thread attachment. Every body is wrapped in try/catch and never throws;
 * failures are logged and reported as `false`.
 */
object SentinelTorchPrimitives {
    private const val TAG = "MobileSentinel.Torch"

    /** First camera id that reports a flash unit. Resolved lazily, cached per-process. */
    @Volatile
    private var torchCameraId: String? = null

    /**
     * Turn the torch on.
     *
     * @return true on success, false if no context, no flash camera, or any error.
     */
    @JvmStatic
    fun turnOn(): Boolean = setTorch(true)

    /**
     * Turn the torch off.
     *
     * @return true on success, false if no context, no flash camera, or any error.
     */
    @JvmStatic
    fun turnOff(): Boolean = setTorch(false)

    /**
     * Whether the device has a camera flash unit.
     *
     * @return true if [PackageManager.FEATURE_CAMERA_FLASH] is present, false on any error.
     */
    @JvmStatic
    fun hasTorch(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_FLASH)
        } catch (e: Throwable) {
            Log.w(TAG, "hasTorch: ${e.message}", e)
            false
        }
    }

    /**
     * Resolve and apply the torch mode on the first flash-capable camera.
     */
    private fun setTorch(enabled: Boolean): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val cameraManager = context.getSystemService(Context.CAMERA_SERVICE) as? CameraManager
                ?: return false
            val id = torchCameraId ?: resolveTorchCameraId(cameraManager)?.also { torchCameraId = it }
            if (id == null) {
                Log.w(TAG, "setTorch($enabled): no flash-capable camera found")
                return false
            }
            cameraManager.setTorchMode(id, enabled)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setTorch($enabled): ${e.message}", e)
            false
        }
    }

    /**
     * Find the first camera id whose [CameraCharacteristics.FLASH_INFO_AVAILABLE] is true.
     * Returns null when the device exposes no flash-capable camera.
     */
    private fun resolveTorchCameraId(cm: CameraManager): String? {
        return try {
            for (id in cm.cameraIdList) {
                val hasFlash = cm.getCameraCharacteristics(id)
                    .get(CameraCharacteristics.FLASH_INFO_AVAILABLE)
                if (hasFlash == true) {
                    return id
                }
            }
            null
        } catch (e: Throwable) {
            Log.w(TAG, "resolveTorchCameraId: ${e.message}", e)
            null
        }
    }
}
