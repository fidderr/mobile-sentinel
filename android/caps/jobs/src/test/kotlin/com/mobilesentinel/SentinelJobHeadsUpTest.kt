package com.mobilesentinel

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.os.SystemClock
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowSystemClock
import java.io.File
import java.time.Duration

/**
 * The guardian's heads-up reaches MAIN's receiver, which takes the ring up and says so; when it
 * does not, the guardian brings the screen up itself.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class SentinelJobHeadsUpTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val app get() = context as Application
    private val screen = "com.example.RingActivity"

    @Before
    fun setUp() {
        // A phone that has been up for an hour: the files these tests write are newer than the boot.
        ShadowSystemClock.advanceBy(Duration.ofHours(1))
        SentinelPrimitives.init(context)
    }

    @After
    fun tearDown() {
        // The tracker is a singleton: no activity stays resumed for the next test.
        SentinelActivityTracker.currentResumedActivity?.let { SentinelActivityTracker.onActivityPaused(it) }
    }

    private fun files() = SentinelStorage.filesDir(context)

    private fun activeJob(id: String): SentinelActiveJobs.ActiveJob {
        val dir = File(files(), "sentinel/jobs")
        dir.mkdirs()
        File(dir, "$id.json").writeText("""{"payload":{"activity_fqcn":"$screen"}}""")
        assertTrue(SentinelActiveJobs.mark(context, id))
        return SentinelActiveJobs.list(context).single { it.id == id }
    }

    private fun taken(id: String) = File(files(), "sentinel/headsup/$id").exists()

    private fun headsUp(id: String) {
        SentinelJobHeadsUpReceiver().onReceive(
            context,
            Intent(SentinelJobGuardian.ACTION_JOB_HEADS_UP).putExtra(SentinelJobGuardian.EXTRA_JOB_ID, id),
        )
    }

    private fun resumeAnActivity() {
        SentinelActivityTracker.onActivityResumed(Robolectric.buildActivity(Activity::class.java).get())
    }

    // ---- MAIN's end ---------------------------------------------------------------------

    @Test
    fun withoutAScreenTheReceiverStartsTheJobsActivityAndSaysSo() {
        activeJob("r1")
        headsUp("r1")
        val started = shadowOf(app).nextStartedActivity
        assertNotNull("no screen was started", started)
        assertEquals(screen, started.component?.className)
        assertTrue(taken("r1"))
    }

    @Test
    fun aSecondHeadsUpDoesNotStartTheScreenAgain() {
        activeJob("r2")
        headsUp("r2")
        assertNotNull(shadowOf(app).nextStartedActivity)
        ShadowSystemClock.advanceBy(Duration.ofMillis(500))
        headsUp("r2")
        assertNull("a second screen was started", shadowOf(app).nextStartedActivity)
        assertTrue(taken("r2"))
    }

    @Test
    fun aHeadsUpWithAScreenInFrontStartsNothingAndSaysSo() {
        activeJob("r3")
        resumeAnActivity()
        headsUp("r3")
        assertNull(shadowOf(app).nextStartedActivity)
        assertTrue(taken("r3"))
    }

    @Test
    fun aJobThatIsNotActiveIsNotTakenUp() {
        headsUp("ghost")
        assertNull(shadowOf(app).nextStartedActivity)
        assertFalse(taken("ghost"))
    }

    @Test
    fun theBackupScreenIsNotCoveredAndNothingIsTakenUp() {
        activeJob("r4")
        SentinelRing.markUiFailed(context)
        headsUp("r4")
        assertNull(shadowOf(app).nextStartedActivity)
        assertFalse(taken("r4"))
        SentinelRing.clearUiFailed(context)
    }

    @Test
    fun aDebugBuildCanHoldTheHeadsUpBackForTheDeviceTests() {
        activeJob("r5")
        File(files(), "sentinel").mkdirs()
        File(files(), "sentinel/debug_hold_heads_up").writeText("")
        app.applicationInfo.flags = app.applicationInfo.flags or ApplicationInfo.FLAG_DEBUGGABLE
        headsUp("r5")
        assertNull(shadowOf(app).nextStartedActivity)
        assertFalse(taken("r5"))
    }

    @Test
    fun aReleaseBuildIgnoresTheSwitch() {
        activeJob("r6")
        File(files(), "sentinel").mkdirs()
        File(files(), "sentinel/debug_hold_heads_up").writeText("")
        app.applicationInfo.flags = app.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE.inv()
        headsUp("r6")
        assertNotNull(shadowOf(app).nextStartedActivity)
        assertTrue(taken("r6"))
    }

    // ---- The guardian's look ------------------------------------------------------------

    private fun look(job: SentinelActiveJobs.ActiveJob) =
        SentinelJobGuardian.checkHeadsUpTaken(context, job, SystemClock.elapsedRealtime())

    /** What the poll loop does after it sent the heads-up. */
    private fun sent(job: SentinelActiveJobs.ActiveJob) {
        SentinelJobGuardian.clearHeadsUpFor(job.id)
        SentinelJobGuardian.sendHeadsUpBroadcast(context, job.id)
        assertTrue(shadowOf(app).broadcastIntents.any { it.action == SentinelJobGuardian.ACTION_JOB_HEADS_UP })
    }

    @Test
    fun beforeTheTimeIsUpTheGuardianWaits() {
        val job = activeJob("g1")
        sent(job)
        ShadowSystemClock.advanceBy(Duration.ofMillis(1_500))
        look(job)
        assertNull(shadowOf(app).nextStartedActivity)
    }

    @Test
    fun aHeadsUpNobodyTookUpGetsTheScreenFromTheGuardianOnce() {
        val job = activeJob("g2")
        sent(job)
        ShadowSystemClock.advanceBy(Duration.ofMillis(2_100))
        look(job)
        val started = shadowOf(app).nextStartedActivity
        assertNotNull("the guardian did not start the screen", started)
        assertEquals(screen, started.component?.className)
        ShadowSystemClock.advanceBy(Duration.ofMillis(2_100))
        look(job)
        assertNull("started twice", shadowOf(app).nextStartedActivity)
    }

    @Test
    fun aHeadsUpMainTookUpIsLeftAlone() {
        val job = activeJob("g3")
        sent(job)
        SentinelJobGuardian.noteHeadsUpTaken(context, job.id)
        ShadowSystemClock.advanceBy(Duration.ofMillis(2_100))
        look(job)
        assertNull(shadowOf(app).nextStartedActivity)
    }

    @Test
    fun aSignFromAnEarlierHeadsUpDoesNotCountForTheNextOne() {
        val job = activeJob("g4")
        SentinelJobGuardian.noteHeadsUpTaken(context, job.id)
        sent(job)
        ShadowSystemClock.advanceBy(Duration.ofMillis(2_100))
        look(job)
        assertNotNull(shadowOf(app).nextStartedActivity)
    }

    @Test
    fun withTheAppsScreenUpTheGuardianDoesNothing() {
        val job = activeJob("g5")
        val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val me = ActivityManager.RunningAppProcessInfo().apply {
            pid = 4242
            processName = context.packageName
        }
        shadowOf(am).setProcesses(listOf(me))
        File(files(), SentinelActivityTracker.APP_RESUMED_FILE).writeText("4242 1")
        sent(job)
        ShadowSystemClock.advanceBy(Duration.ofMillis(2_100))
        look(job)
        assertNull(shadowOf(app).nextStartedActivity)
    }

    @Test
    fun aRingThatEndedIsNotGivenAScreen() {
        val job = activeJob("g6")
        sent(job)
        File(files(), "sentinel/jobs/g6.limit").writeText("")
        ShadowSystemClock.advanceBy(Duration.ofMillis(2_100))
        look(job)
        assertNull(shadowOf(app).nextStartedActivity)
    }
}
