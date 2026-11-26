package com.mobilesentinel

import android.Manifest
import android.app.Activity
import android.app.Application
import android.content.Context
import android.os.Environment
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.android.controller.ActivityController
import org.robolectric.annotation.Config
import java.io.File
import java.util.concurrent.atomic.AtomicReference

/**
 * Saving to Downloads: MediaStore from Android 10, the public folder (with the storage permission, a dialog that is asked for and
 * never waited for) before it.
 */
@Suppress("DEPRECATION")
@RunWith(RobolectricTestRunner::class)
class SentinelSharePrimitivesTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val app get() = context as Application
    private val permission = Manifest.permission.WRITE_EXTERNAL_STORAGE
    private lateinit var source: File
    private var controller: ActivityController<Activity>? = null

    @Before
    fun setUp() {
        SentinelPrimitives.init(context)
        SentinelActivityTracker.attach(app)
        source = File(context.cacheDir, "backup.json").apply { writeText("""{"alarms":[]}""") }
    }

    @After
    fun tearDown() {
        controller?.pause()
    }

    private fun downloads() = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)

    private fun resumedActivity(): Activity {
        val c = Robolectric.buildActivity(Activity::class.java).setup()
        controller = c
        return c.get()
    }

    @Test
    @Config(sdk = [26, 28])
    fun below29WithThePermissionTheFileLandsInTheDownloadsFolder() {
        shadowOf(app).grantPermissions(permission)
        assertEquals("Download/b.json", SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json"))
        assertEquals("""{"alarms":[]}""", File(downloads(), "b.json").readText())
    }

    @Test
    @Config(sdk = [26, 28])
    fun below29WithoutThePermissionNothingIsWritten() {
        assertEquals("", SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json"))
        assertFalse(File(downloads(), "b.json").exists())
    }

    /** The dialog is asked for through the activity, and the call says "not saved" at once. */
    @Test
    @Config(sdk = [26, 28])
    fun below29WithoutThePermissionTheDialogIsAskedFor() {
        val activity = resumedActivity()
        assertEquals("", SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json"))
        val request = shadowOf(activity).lastRequestedPermission
        assertNotNull("the storage permission was not asked for", request)
        assertEquals(listOf(permission), request.requestedPermissions.toList())
    }

    /**
     * Never waits for the dialog, on any thread: the native UI thread of an app is not the Java main looper, and the dialog
     * pauses the activity, which the main thread then holds until that thread answers (the app froze for the whole wait).
     */
    @Test
    @Config(sdk = [26, 28])
    fun below29OnAnotherThreadItAsksAndReturnsAtOnce() {
        val activity = resumedActivity()
        val saved = AtomicReference<String?>()
        val worker = Thread { saved.set(SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json")) }
        val begun = System.nanoTime()
        worker.start()
        worker.join(2_000)
        assertFalse("the call is still waiting after ${(System.nanoTime() - begun) / 1_000_000} ms", worker.isAlive)
        assertEquals("", saved.get())
        org.robolectric.shadows.ShadowLooper.idleMainLooper()
        assertNotNull("the storage permission was not asked for", shadowOf(activity).lastRequestedPermission)
        assertFalse(File(downloads(), "b.json").exists())
    }

    /** The next try, after the person allowed it, saves. */
    @Test
    @Config(sdk = [26, 28])
    fun below29TheTryAfterTheAnswerSaves() {
        resumedActivity()
        assertEquals("", SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json"))
        shadowOf(app).grantPermissions(permission)
        assertEquals("Download/b.json", SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json"))
        assertTrue(File(downloads(), "b.json").exists())
    }

    /** The query a host asks after a failed save: the permission is what is missing, up to Android 9 only. */
    @Test
    @Config(sdk = [26, 28])
    fun below29TheQueryNamesTheMissingPermission() {
        assertTrue(SentinelSharePrimitives.downloadsPermissionMissing())
        shadowOf(app).grantPermissions(permission)
        assertFalse(SentinelSharePrimitives.downloadsPermissionMissing())
    }

    @Test
    @Config(sdk = [29, 33])
    fun from29TheQueryNeverNamesAPermission() {
        assertFalse(SentinelSharePrimitives.downloadsPermissionMissing())
    }

    /** The query only reads: it never opens the dialog. */
    @Test
    @Config(sdk = [28])
    fun theQueryDoesNotAsk() {
        val activity = resumedActivity()
        SentinelSharePrimitives.downloadsPermissionMissing()
        org.robolectric.shadows.ShadowLooper.idleMainLooper()
        assertNull(shadowOf(activity).lastRequestedPermission)
    }

    private fun save() = SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json")

    private fun rationale(shown: Boolean) =
        shadowOf(context.packageManager).setShouldShowRequestPermissionRationale(permission, shown)

    /** The first ask shows the dialog, so nothing is blocked; the answer "no" without "never ask again" keeps the dialog coming. */
    @Test
    @Config(sdk = [26, 28])
    fun below29AFirstAskAndASimpleNoStayAsksAgain() {
        val activity = resumedActivity()
        assertEquals("", save())
        assertFalse("the first ask is not blocked", SentinelSharePrimitives.downloadsPermissionBlocked())
        org.robolectric.shadows.ShadowLooper.idleMainLooper()
        val first = shadowOf(activity).lastRequestedPermission
        assertNotNull(first)
        rationale(true)
        assertEquals("", save())
        assertFalse("a simple no is not blocked", SentinelSharePrimitives.downloadsPermissionBlocked())
        org.robolectric.shadows.ShadowLooper.idleMainLooper()
        assertNotSame("the dialog was not asked for again", first, shadowOf(activity).lastRequestedPermission)
    }

    /** Asked before and Android gives no rationale: "never ask again". No dialog is requested, the query says blocked. */
    @Test
    @Config(sdk = [26, 28])
    fun below29AfterNeverAskAgainTheSaveSaysBlockedAndAsksNothing() {
        val activity = resumedActivity()
        assertEquals("", save())
        org.robolectric.shadows.ShadowLooper.idleMainLooper()
        val first = shadowOf(activity).lastRequestedPermission
        rationale(false)
        assertEquals("", save())
        assertTrue(SentinelSharePrimitives.downloadsPermissionBlocked())
        assertTrue(SentinelSharePrimitives.downloadsPermissionMissing())
        org.robolectric.shadows.ShadowLooper.idleMainLooper()
        assertSame("a request that cannot show a dialog was made", first, shadowOf(activity).lastRequestedPermission)
    }

    /** Allowed in the settings afterwards: saved, and no longer blocked. */
    @Test
    @Config(sdk = [26, 28])
    fun below29AllowedInTheSettingsTheSaveWorks() {
        resumedActivity()
        assertEquals("", save())
        rationale(false)
        assertEquals("", save())
        assertTrue(SentinelSharePrimitives.downloadsPermissionBlocked())
        shadowOf(app).grantPermissions(permission)
        assertFalse(SentinelSharePrimitives.downloadsPermissionBlocked())
        assertEquals("Download/b.json", save())
        assertFalse(SentinelSharePrimitives.downloadsPermissionBlocked())
    }

    @Test
    @Config(sdk = [29, 33])
    fun from29NothingIsEverBlocked() {
        resumedActivity()
        save()
        rationale(false)
        save()
        assertFalse(SentinelSharePrimitives.downloadsPermissionBlocked())
    }

    /**
     * Rust calls these by name as static methods (`call_static_method`). A member of a Kotlin `object` without @JvmStatic is
     * none, and the JNI call then ends the process (a pending NoSuchMethodError).
     */
    @Test
    @Config(sdk = [28])
    fun everyEntryPointRustCallsIsStatic() {
        val names = listOf(
            "openFile", "shareEmail", "openUrl", "saveToDownloads", "shareText", "shareUrl", "shareFile",
            "downloadsPermissionMissing",
        )
        for (name in names) {
            val methods = SentinelSharePrimitives::class.java.methods.filter { it.name == name }
            assertTrue("no method $name", methods.isNotEmpty())
            assertTrue("$name is not static", methods.any { java.lang.reflect.Modifier.isStatic(it.modifiers) })
        }
    }

    /** From Android 10 there is no storage permission to ask for. */
    @Test
    @Config(sdk = [29, 33])
    fun from29TheStoragePermissionIsNeverAskedFor() {
        val activity = resumedActivity()
        SentinelSharePrimitives.saveToDownloads(source.path, "b.json", "application/json")
        assertNull(shadowOf(activity).lastRequestedPermission)
        assertFalse(File(downloads(), "b.json").exists())
    }
}
