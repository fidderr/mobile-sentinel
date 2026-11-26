package com.mobilesentinel

import android.Manifest
import android.app.Application
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** MediaRecorder takes a Context only from Android 12: before it the plain constructor has to be used. */
@RunWith(RobolectricTestRunner::class)
class SentinelRecorderPrimitivesTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val dir get() = java.io.File(context.cacheDir, "notes").path

    @Before
    fun setUp() {
        SentinelPrimitives.init(context)
    }

    @After
    fun tearDown() {
        SentinelRecorderPrimitives.cancel()
    }

    @Test
    @Config(sdk = [26, 28, 30, 31, 33, 36])
    fun recordingStartsWithThePermissionOnEveryVersion() {
        shadowOf(context as Application).grantPermissions(Manifest.permission.RECORD_AUDIO)
        assertEquals(0, SentinelRecorderPrimitives.start(dir))
        assertTrue(SentinelRecorderPrimitives.isRecording())
        SentinelRecorderPrimitives.cancel()
        assertEquals(false, SentinelRecorderPrimitives.isRecording())
    }

    @Test
    @Config(sdk = [28, 33])
    fun withoutThePermissionNothingStarts() {
        assertEquals(1, SentinelRecorderPrimitives.start(dir))
        assertEquals(false, SentinelRecorderPrimitives.isRecording())
    }
}
