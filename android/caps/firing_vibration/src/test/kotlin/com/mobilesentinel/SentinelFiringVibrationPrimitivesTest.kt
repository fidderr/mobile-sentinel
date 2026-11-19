package com.mobilesentinel

import android.content.Context
import android.media.AudioAttributes
import android.os.Vibrator
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** The alarm buzz, on the two ways Android hands out a vibrator and the two ways to attribute it. */
@RunWith(RobolectricTestRunner::class)
class SentinelFiringVibrationPrimitivesTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val pattern = longArrayOf(0, 400, 200, 400)

    @Before
    fun setUp() {
        SentinelPrimitives.init(context)
    }

    private fun vibrator(): Vibrator = SentinelCompat.vibrator(context)!!

    @Test
    @Config(sdk = [26, 28, 29, 30, 31, 33, 34, 36])
    fun startVibratesAndStopCancels() {
        assertTrue(SentinelFiringVibrationPrimitives.startVibration(pattern))
        assertTrue(shadowOf(vibrator()).isVibrating)
        assertTrue(SentinelFiringVibrationPrimitives.stopVibration())
        assertFalse(shadowOf(vibrator()).isVibrating)
    }

    @Test
    @Config(sdk = [26, 33])
    fun anEmptyPatternMeansNoVibration() {
        assertFalse(SentinelFiringVibrationPrimitives.startVibration(longArrayOf()))
        assertFalse(shadowOf(vibrator()).isVibrating)
    }

    /** Below Android 13 the alarm usage travels as audio attributes. */
    @Test
    @Config(sdk = [26, 30])
    fun belowAndroid13TheUsageIsAnAudioAttribute() {
        SentinelFiringVibrationPrimitives.startVibration(pattern)
        assertEquals(
            AudioAttributes.USAGE_ALARM,
            shadowOf(vibrator()).audioAttributesFromLastVibration!!.usage,
        )
    }

    /** From Android 13 it is a VibrationAttributes: the old call would be deprecated, the new one is needed. */
    @Test
    @Config(sdk = [33, 34, 36])
    fun fromAndroid13TheUsageIsAVibrationAttribute() {
        SentinelFiringVibrationPrimitives.startVibration(pattern)
        assertNotNull(shadowOf(vibrator()).vibrationAttributesFromLastVibration)
    }
}
