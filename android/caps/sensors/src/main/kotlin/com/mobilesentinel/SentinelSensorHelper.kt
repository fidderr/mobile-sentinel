package com.mobilesentinel

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.util.Log

/**
 * Kotlin helper for device sensor access (accelerometer, step counter).
 * Registers SensorEventListeners and forwards data to Rust via SentinelBridge.
 */
object SentinelSensorHelper {

    private const val TAG = "MobileSentinel.Sensor"

    private var sensorManager: SensorManager? = null
    private var accelListener: SensorEventListener? = null
    private var stepListener: SensorEventListener? = null
    private var stepCountAtStart: Int = -1

    /**
     * Initialize the sensor manager from the app context.
     *
     * Optional: callers may invoke this eagerly, but the helper also
     * lazily resolves the SensorManager from [SentinelPrimitives.getAppContext]
     * on first use. Lazy init keeps the always-on core (the library
     * initializer) from referencing this capability module — sensors live in
     * their own Gradle module and core must not depend on it.
     */
    @JvmStatic
    fun init(context: Context) {
        sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
        Log.i(TAG, "SensorManager initialized: ${sensorManager != null}")
    }

    /** Resolve the SensorManager lazily from the app context if not set. */
    private fun manager(): SensorManager? {
        sensorManager?.let { return it }
        val ctx = SentinelPrimitives.getAppContext() ?: return null
        val sm = ctx.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
        sensorManager = sm
        return sm
    }

    /**
     * Start accelerometer updates. Delivers (x, y, z) to Rust via bridge.
     */
    @JvmStatic
    fun startAccelerometer(): Boolean {
        val sm = manager() ?: return false
        val sensor = sm.getDefaultSensor(Sensor.TYPE_ACCELEROMETER) ?: return false

        accelListener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                val x = event.values[0].toDouble()
                val y = event.values[1].toDouble()
                val z = event.values[2].toDouble()
                SentinelBridge.onAccelerometerData(x, y, z)
            }

            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
        }

        sm.registerListener(accelListener, sensor, SensorManager.SENSOR_DELAY_GAME)
        Log.i(TAG, "Accelerometer started")
        return true
    }

    /**
     * Stop accelerometer updates.
     */
    @JvmStatic
    fun stopAccelerometer() {
        accelListener?.let {
            sensorManager?.unregisterListener(it)
            accelListener = null
            Log.i(TAG, "Accelerometer stopped")
        }
    }

    /**
     * Start step counter. Uses TYPE_STEP_COUNTER (cumulative since boot)
     * as primary — most reliable across devices. Falls back to TYPE_STEP_DETECTOR.
     * Requires ACTIVITY_RECOGNITION runtime permission on Android 10+.
     * If registration fails (permission not yet granted), retries after a delay.
     */
    @JvmStatic
    fun startStepCounter(): Boolean {
        val sm = manager() ?: run {
            Log.e(TAG, "SensorManager is null — no app context available")
            return false
        }

        // TYPE_STEP_COUNTER is more reliable (cumulative, always delivers)
        val sensor = sm.getDefaultSensor(Sensor.TYPE_STEP_COUNTER)
            ?: sm.getDefaultSensor(Sensor.TYPE_STEP_DETECTOR)

        if (sensor == null) {
            Log.w(TAG, "No step sensor available on this device")
            return false
        }

        val useDetector = sensor.type == Sensor.TYPE_STEP_DETECTOR
        stepCountAtStart = if (useDetector) 0 else -1

        stepListener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                if (useDetector) {
                    // STEP_DETECTOR fires once per step
                    stepCountAtStart++
                    SentinelBridge.onStepCount(stepCountAtStart)
                } else {
                    // STEP_COUNTER is cumulative since boot
                    val totalSteps = event.values[0].toInt()
                    if (stepCountAtStart < 0) {
                        stepCountAtStart = totalSteps
                    }
                    val relativeSteps = totalSteps - stepCountAtStart
                    if (relativeSteps >= 0) {
                        SentinelBridge.onStepCount(relativeSteps)
                    }
                }
            }

            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
        }

        // Try to register — may fail if permission not yet granted
        var registered = sm.registerListener(stepListener, sensor, SensorManager.SENSOR_DELAY_FASTEST)
        if (!registered) {
            Log.w(TAG, "Step counter registration failed — retrying in 2s (permission may be pending)")
            // Retry after delay (permission dialog may be showing)
            Thread {
                Thread.sleep(2000)
                val retryRegistered = sm.registerListener(stepListener, sensor, SensorManager.SENSOR_DELAY_FASTEST)
                Log.i(TAG, "Step counter retry: registered=$retryRegistered")
                if (!retryRegistered) {
                    // One more retry after 5s
                    Thread.sleep(5000)
                    val finalRetry = sm.registerListener(stepListener, sensor, SensorManager.SENSOR_DELAY_FASTEST)
                    Log.i(TAG, "Step counter final retry: registered=$finalRetry")
                }
            }.start()
        }

        Log.i(TAG, "Step counter started (type=${sensor.stringType}, registered=$registered)")
        return true // Return true since we'll retry
    }

    /**
     * Stop step counter.
     */
    @JvmStatic
    fun stopStepCounter() {
        stepListener?.let {
            sensorManager?.unregisterListener(it)
            stepListener = null
            stepCountAtStart = -1
            Log.i(TAG, "Step counter stopped")
        }
    }
}
