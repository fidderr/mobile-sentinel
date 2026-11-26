package com.mobilesentinel.smoke

import android.app.Activity
import android.app.ActivityManager
import android.app.AlarmManager
import android.app.KeyguardManager
import android.content.ContentUris
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.os.PowerManager
import android.provider.MediaStore
import android.view.View
import android.view.WindowManager
import androidx.test.platform.app.InstrumentationRegistry
import com.mobilesentinel.SentinelAudioPreviewPrimitives
import com.mobilesentinel.SentinelCompat
import com.mobilesentinel.SentinelExactAlarmPrimitives
import com.mobilesentinel.SentinelFiringVibrationPrimitives
import com.mobilesentinel.SentinelKioskController
import com.mobilesentinel.SentinelPrimitives
import com.mobilesentinel.SentinelRing
import com.mobilesentinel.SentinelSharePrimitives
import com.mobilesentinel.SentinelStorage
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * What the SDK does on a real Android runtime: the paths Robolectric can only imitate. A run of this class takes about a
 * minute. Needs the storage permission granted at install (`adb install -g`), which Android 9 asks for to save a file.
 */
class DeviceSmokeTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context: Context get() = instrumentation.targetContext
    private val alarmId = "smoke-alarm"

    private fun shell(command: String): String {
        val fd = instrumentation.uiAutomation.executeShellCommand(command)
        return ParcelFileDescriptor.AutoCloseInputStream(fd).use { String(it.readBytes()) }
    }

    private fun <T : Any> waitFor(what: String, seconds: Int, read: () -> T?): T {
        val end = System.nanoTime() + seconds * 1_000_000_000L
        while (System.nanoTime() < end) {
            read()?.let { return it }
            Thread.sleep(200)
        }
        throw AssertionError("gave up after ${seconds}s: $what")
    }

    private fun sentinelDir() = File(SentinelStorage.filesDir(context), "sentinel")

    /** The ring's timing log (written in :sentinel), lines of "<epoch ms> <event> <id>": the last time of an event. */
    private fun timing(event: String, id: String): Long? =
        File(sentinelDir(), "ringing/timing.log").takeIf { it.exists() }?.readLines()
            ?.mapNotNull { line ->
                line.split(" ").takeIf { it.size == 3 && it[1] == event && it[2] == id }?.get(0)?.toLongOrNull()
            }
            ?.lastOrNull()

    @Suppress("DEPRECATION")
    private fun ringServiceForeground(): Boolean =
        (context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getRunningServices(100)
            .any { it.service.className == "com.mobilesentinel.SentinelForegroundService" && it.foreground }

    @Before
    fun setUp() {
        SentinelPrimitives.init(context)
        resetRing()
    }

    @After
    fun tearDown() {
        resetRing()
        SentinelKioskController.disable()
        SmokeScreens.resumed?.finish()
        shell("input keyevent KEYCODE_WAKEUP")
    }

    private fun resetRing() {
        SentinelRing.stop(context, alarmId)
        SentinelExactAlarmPrimitives.cancelExactAlarm(alarmId)
        for (d in listOf("jobs", "ringing", "active")) sentinelDir().resolve(d).deleteRecursively()
    }

    /** A tone the media player can open: 16-bit mono PCM, half a second of 440 Hz. */
    private fun writeTone(): File {
        val rate = 8000
        val samples = rate / 2
        val data = ByteBuffer.allocate(samples * 2).order(ByteOrder.LITTLE_ENDIAN)
        for (i in 0 until samples) data.putShort((Math.sin(2 * Math.PI * 440 * i / rate) * 8000).toInt().toShort())
        val out = ByteArrayOutputStream()
        fun int(v: Int) = out.write(ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(v).array())
        fun short(v: Int) = out.write(ByteBuffer.allocate(2).order(ByteOrder.LITTLE_ENDIAN).putShort(v.toShort()).array())
        out.write("RIFF".toByteArray())
        int(36 + data.capacity())
        out.write("WAVEfmt ".toByteArray())
        int(16); short(1); short(1); int(rate); int(rate * 2); short(2); short(16)
        out.write("data".toByteArray())
        int(data.capacity())
        out.write(data.array())
        return File(context.cacheDir, "tone.wav").apply { writeBytes(out.toByteArray()) }
    }

    @Test
    fun anExactAlarmIsArmedFiresAndTheRingServiceStartsInTheForeground() {
        val plan = JSONObject()
            .put("instance_id", alarmId).put("title", "Smoke").put("body", "smoke")
            .put("channel_id", "smoke").put("channel_name", "Smoke").put("importance", 5)
            .put("sound_uri", writeTone().path).put("looping", true).put("vibrate", true)
            .put("firing_full_screen", false).put("kiosk_mode", false)
            .toString()
        File(sentinelDir(), "jobs").apply { mkdirs() }.resolve("$alarmId.json")
            .writeText(JSONObject().put("id", alarmId).toString())

        val at = System.currentTimeMillis() + 4_000
        assertTrue("the alarm was refused", SentinelExactAlarmPrimitives.scheduleExactAlarm(alarmId, at, plan))
        val alarmClock = (context.getSystemService(Context.ALARM_SERVICE) as AlarmManager).nextAlarmClock
        assertNotNull("the system holds no alarm clock", alarmClock)
        assertEquals("the alarm is armed for another time", at, alarmClock!!.triggerTime)

        val delivered = waitFor("the alarm reaches the receiver", 40) { timing("delivered", alarmId) }
        assertTrue("delivered ${at - delivered} ms before its time", delivered >= at - 500)
        assertTrue("delivered ${delivered - at} ms late", delivered - at < 20_000)
        waitFor("the ring service starts", 20) { timing("service", alarmId) }
        waitFor("the ring service runs in the foreground", 20) { if (ringServiceForeground()) true else null }
        waitFor("the sound starts", 20) { timing("sound", alarmId) }
        Thread.sleep(2_000)
        assertNull("no sound could be played", timing("no_sound", alarmId))
        assertTrue("the ring service left the foreground", ringServiceForeground())
        assertEquals(alarmId, SentinelRing.list(context).single().id)
    }

    @Test
    fun vibrationAndAudioCallsReturn() {
        assertNotNull("the vibrator service is missing", SentinelCompat.vibrator(context))
        assertTrue(SentinelFiringVibrationPrimitives.startVibration(longArrayOf(0, 200, 100, 200)))
        assertTrue(SentinelFiringVibrationPrimitives.stopVibration())

        val handle = SentinelAudioPreviewPrimitives.play(writeTone().path, false, true)
        assertTrue("the tone did not play", handle > 0)
        assertTrue(SentinelAudioPreviewPrimitives.setVolume(0.5f))
        assertTrue(SentinelAudioPreviewPrimitives.stop(handle))
    }

    @Test
    fun aFileIsSavedToDownloads() {
        val name = "sentinel-smoke-${System.currentTimeMillis()}.json"
        val body = """{"smoke":true}"""
        val source = File(context.cacheDir, "smoke.json").apply { writeText(body) }
        assertEquals("Download/$name", SentinelSharePrimitives.saveToDownloads(source.path, name, "application/json"))
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val resolver = context.contentResolver
            val collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI
            val id = resolver.query(
                collection, arrayOf(MediaStore.Downloads._ID),
                "${MediaStore.Downloads.DISPLAY_NAME}=?", arrayOf(name), null,
            ).use { c -> if (c != null && c.moveToFirst()) c.getLong(0) else null }
            assertNotNull("MediaStore has no $name", id)
            val uri = ContentUris.withAppendedId(collection, id!!)
            assertEquals(body, resolver.openInputStream(uri)!!.use { String(it.readBytes()) })
            resolver.delete(uri, null, null)
        } else {
            @Suppress("DEPRECATION")
            val saved = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), name)
            assertEquals(body, saved.readText())
            saved.delete()
        }
    }

    @Test
    fun theKioskLockAppliesToAWindowOnEveryVersion() {
        SentinelKioskController.enable(
            activityFqcn = SmokeActivity::class.java.name,
            blockHome = true,
            blockBack = true,
            blockRecents = true,
            relaunchDebounceMs = 200,
            hideStatusBar = true,
            hideNavBar = true,
        )
        context.startActivity(Intent(context, SmokeActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        val activity = waitFor("the smoke screen resumes", 30) { SmokeScreens.resumed }
        // The lock ends its window work with this flag: a call newer than the device would throw before it.
        waitFor("the kiosk lock applies to the window", 15) {
            if (activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON != 0) true else null
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            @Suppress("DEPRECATION")
            val visibility = activity.window.decorView.systemUiVisibility
            assertTrue("the bars were not hidden with the system UI flags", visibility and View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY != 0)
        }
    }

    /** What the system says about the screen, the lock screen and the focus: the message of a failure here. */
    private fun screenState(): String =
        shell("dumpsys power").lines().filter { "mWakefulness=" in it || "Display Power" in it }.joinToString(" | ") { it.trim() } +
            " || " + shell("dumpsys window").lines()
            .filter { "mCurrentFocus" in it || "isKeyguardShowing" in it || "mShowingLockscreen" in it || "mOccluded" in it || "mKeyguardOccluded" in it }
            .joinToString(" | ") { it.trim() } + " || " + shell("dumpsys activity activities").lines()
            .filter { "mResumedActivity" in it || "mLastPausedActivity" in it }.joinToString(" | ") { it.trim() }

    private fun lockPhone(): Pair<PowerManager, KeyguardManager> {
        val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        val keyguard = context.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
        shell("locksettings set-disabled false")
        shell("input keyevent KEYCODE_SLEEP")
        waitFor("the screen turns off", 20) { if (!power.isInteractive) true else null }
        waitFor("the phone is asleep", 20) { if ("mWakefulness=Asleep" in shell("dumpsys power")) true else null }
        waitFor("the lock screen comes up", 20) { if (keyguard.isKeyguardLocked) true else null }
        return power to keyguard
    }

    /**
     * Waits for what the system guarantees once a window is over the lock screen: the window is the one in front (resumed,
     * with the focus) and the lock screen has been set aside for it (`mKeyguardOccluded=true`; the system takes a moment
     * to finish this after the window is resumed).
     */
    private fun waitOverLockScreen(before: Int, what: String): Activity {
        try {
            waitFor("$what comes to the front", 40) { if (SmokeScreens.resumes.get() > before) true else null }
            val activity = waitFor("$what has the focus", 20) { SmokeScreens.resumed?.takeIf { it.hasWindowFocus() } }
            waitFor("$what has the lock screen set aside", 20) {
                if ("mKeyguardOccluded=true" in shell("dumpsys window")) true else null
            }
            return activity
        } catch (e: AssertionError) {
            throw AssertionError("${e.message}; ${screenState()}")
        }
    }

    private fun openSmokeScreen(): Activity {
        val before = SmokeScreens.resumes.get()
        context.startActivity(Intent(context, SmokeActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        return waitOverLockScreen(before, "the smoke screen")
    }

    /** The lock screen is up and the screen is on, as when a person picks the phone up, and a window opens over it. */
    @Test
    fun aWindowOpensOverTheLockScreen() {
        val (power, keyguard) = lockPhone()
        shell("input keyevent KEYCODE_WAKEUP")
        waitFor("the screen turns on", 20) { if (power.isInteractive) true else null }
        waitFor("the lock screen is up", 20) { if (keyguard.isKeyguardLocked) true else null }
        openSmokeScreen()
        assertTrue("the window opened after the phone was unlocked, not over the lock screen", keyguard.isKeyguardLocked)
    }

    /** What a ring does: the phone sleeps behind its lock screen, the alarm's full-screen intent opens the window and turns the screen on. */
    @Test
    fun aRingOpensItsScreenOverTheLockScreen() {
        val (power, keyguard) = lockPhone()
        val plan = JSONObject()
            .put("instance_id", alarmId).put("title", "Smoke").put("body", "smoke")
            .put("channel_id", "smoke").put("channel_name", "Smoke").put("importance", 5)
            .put("sound_uri", "silent://").put("vibrate", false)
            .put("activity_fqcn", SmokeRingActivity::class.java.name).put("firing_full_screen", true).put("kiosk_mode", false)
            .toString()
        File(sentinelDir(), "jobs").apply { mkdirs() }.resolve("$alarmId.json")
            .writeText(JSONObject().put("id", alarmId).toString())
        val before = SmokeScreens.resumes.get()
        assertTrue(SentinelExactAlarmPrimitives.scheduleExactAlarm(alarmId, System.currentTimeMillis() + 3_000, plan))
        waitOverLockScreen(before, "the ring's screen")
        waitFor("the screen turns on", 20) { if (power.isInteractive) true else null }
        assertTrue("the window opened after the phone was unlocked, not over the lock screen", keyguard.isKeyguardLocked)
    }
}
