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
    /** The step counter's reading when counting started, -1 before its first event. */
    private var stepCountAtStart: Int = -1
    /** Steps the detector reported, and steps the counter has advanced by. */
    private var detectedSteps: Int = 0
    private var countedSteps: Int = 0

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
     * Start counting steps. TYPE_STEP_DETECTOR reports a step as it is taken; TYPE_STEP_COUNTER
     * is batched in hardware and can run seconds behind, but exists on more phones and misses
     * nothing. Both are read where both exist and the higher count is reported, so a step
     * shows at once and none is lost. Requires ACTIVITY_RECOGNITION on Android 10+.
     * If registration fails (permission not yet granted), it is retried after a delay.
     */
    @JvmStatic
    fun startStepCounter(): Boolean {
        val sm = manager() ?: run {
            Log.e(TAG, "SensorManager is null — no app context available")
            return false
        }

        val detector = sm.getDefaultSensor(Sensor.TYPE_STEP_DETECTOR)
        val counter = sm.getDefaultSensor(Sensor.TYPE_STEP_COUNTER)
        val sensors = listOfNotNull(detector, counter)
        if (sensors.isEmpty()) {
            Log.w(TAG, "No step sensor available on this device")
            return false
        }

        stopStepCounter()
        stepCountAtStart = -1
        detectedSteps = 0
        countedSteps = 0

        val listener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                when (event.sensor.type) {
                    Sensor.TYPE_STEP_DETECTOR -> detectedSteps += 1
                    Sensor.TYPE_STEP_COUNTER -> {
                        // Cumulative since boot: the first reading is the zero.
                        val total = event.values[0].toInt()
                        if (stepCountAtStart < 0) {
                            stepCountAtStart = total
                        }
                        countedSteps = (total - stepCountAtStart).coerceAtLeast(0)
                    }
                    else -> return
                }
                SentinelBridge.onStepCount(maxOf(detectedSteps, countedSteps))
            }

            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
        }
        stepListener = listener

        // No report latency: a batched step is a late step.
        fun register(): Boolean = sensors
            .map { sm.registerListener(listener, it, SensorManager.SENSOR_DELAY_UI, 0) }
            .all { it }

        val registered = register()
        if (!registered) {
            Log.w(TAG, "Step sensor registration failed — retrying in 2s (permission may be pending)")
            // Retry after delay (permission dialog may be showing)
            Thread {
                Thread.sleep(2000)
                val retryRegistered = register()
                Log.i(TAG, "Step sensor retry: registered=$retryRegistered")
                if (!retryRegistered) {
                    // One more retry after 5s
                    Thread.sleep(5000)
                    val finalRetry = register()
                    Log.i(TAG, "Step sensor final retry: registered=$finalRetry")
                }
            }.start()
        }

        Log.i(TAG, "Step counting started (detector=${detector != null}, counter=${counter != null}, registered=$registered)")
        return true // Return true since we'll retry
    }

    /**
     * Stop counting steps.
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
