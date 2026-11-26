package com.mobilesentinel

import android.app.Activity
import android.view.KeyEvent
import android.view.View
import android.view.Window
import android.view.WindowManager
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** The lock on the alarm screen: bars hidden the old way before Android 11, Back and volume keys swallowed. */
@Suppress("DEPRECATION")
@RunWith(RobolectricTestRunner::class)
class SentinelKioskControllerTest {
    private val controller = SentinelKioskController

    @After
    fun tearDown() {
        controller.disable()
        controller.currentResumedActivity = null
    }

    /** The keys that got through to the activity's own window callback, which the lock sits in front of. */
    private val reached = mutableListOf<Int>()

    private fun enable(statusBar: Boolean, navBar: Boolean): Activity {
        val activity = Robolectric.buildActivity(Activity::class.java).create().start().resume().get()
        val own = activity.window.callback
        activity.window.callback = object : Window.Callback by own {
            override fun dispatchKeyEvent(event: KeyEvent): Boolean {
                reached.add(event.keyCode)
                return false
            }
        }
        controller.enable(
            activityFqcn = Activity::class.java.name,
            blockHome = true,
            blockBack = true,
            blockRecents = true,
            relaunchDebounceMs = 200,
            hideStatusBar = statusBar,
            hideNavBar = navBar,
        )
        controller.onActivityCreated(activity, null)
        controller.onActivityResumed(activity)
        return activity
    }

    private fun visibility(activity: Activity) = activity.window.decorView.systemUiVisibility

    /** Android 9 and 10 have no insets controller: the bars go with the system UI flags. */
    @Test
    @Config(sdk = [26, 28, 29])
    fun beforeAndroid11TheBarsAreHiddenWithSystemUiFlags() {
        val v = visibility(enable(statusBar = true, navBar = true))
        assertTrue(v and View.SYSTEM_UI_FLAG_FULLSCREEN != 0)
        assertTrue(v and View.SYSTEM_UI_FLAG_HIDE_NAVIGATION != 0)
        assertTrue(v and View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY != 0)
    }

    @Test
    @Config(sdk = [28])
    fun beforeAndroid11OnlyTheAskedBarsAreHidden() {
        val v = visibility(enable(statusBar = true, navBar = false))
        assertTrue(v and View.SYSTEM_UI_FLAG_FULLSCREEN != 0)
        assertEquals(0, v and View.SYSTEM_UI_FLAG_HIDE_NAVIGATION)
        val w = visibility(enable(statusBar = false, navBar = true))
        assertEquals(0, w and View.SYSTEM_UI_FLAG_FULLSCREEN)
        assertTrue(w and View.SYSTEM_UI_FLAG_HIDE_NAVIGATION != 0)
    }

    /** From Android 11 the insets controller does it, and the old flags stay unused. */
    @Test
    @Config(sdk = [30, 33])
    fun fromAndroid11TheSystemUiFlagsStayUnused() {
        assertEquals(0, visibility(enable(statusBar = true, navBar = true)) and View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY)
    }

    @Test
    @Config(sdk = [28, 33])
    fun theAlarmScreenKeepsTheScreenOnAndShowsOverTheLockScreen() {
        val activity = enable(statusBar = true, navBar = true)
        assertTrue(activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON != 0)
        controller.disable()
        assertEquals(0, activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    /** Before Android 13 Back arrives as a key event: the window callback swallows it while the lock is on. */
    @Test
    @Config(sdk = [26, 28, 30, 33])
    fun theBackAndVolumeKeysAreSwallowedWhileLocked() {
        val activity = enable(statusBar = true, navBar = true)
        for (key in listOf(KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN)) {
            for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
                assertTrue("key $key was not consumed", activity.window.callback.dispatchKeyEvent(KeyEvent(action, key)))
            }
        }
        assertEquals(emptyList<Int>(), reached)
    }

    @Test
    @Config(sdk = [28, 33])
    fun otherKeysAndAllKeysWithoutTheLockReachTheActivity() {
        val activity = enable(statusBar = true, navBar = true)
        activity.window.callback.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_A))
        controller.disable()
        activity.window.callback.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK))
        assertEquals(listOf(KeyEvent.KEYCODE_A, KeyEvent.KEYCODE_BACK), reached)
    }

    /**
     * A window call the phone does not have throws an Error (NoSuchMethodError), which `catch (e: Exception)` lets through:
     * the set-up stopped there and the key guard behind it was never put in. The lock is switched on while the activity is
     * already showing (so `onActivityCreated` did not install the guard), and one call of `engageOn` throws such an Error.
     */
    private fun engageOnAMissingCall(activityClass: Class<out Activity>) {
        val activity = Robolectric.buildActivity(activityClass).create().start().resume().get()
        val own = activity.window.callback
        activity.window.callback = object : Window.Callback by own {
            override fun dispatchKeyEvent(event: KeyEvent): Boolean {
                reached.add(event.keyCode)
                return false
            }
        }
        controller.enable(
            activityFqcn = activityClass.name,
            blockHome = true,
            blockBack = true,
            blockRecents = true,
            relaunchDebounceMs = 200,
            hideStatusBar = true,
            hideNavBar = true,
        )
        controller.onActivityResumed(activity)
        assertTrue(
            "Back was not consumed: the key guard is missing",
            activity.window.callback.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK)),
        )
        assertEquals(emptyList<Int>(), reached)
    }

    /** The first call of the set-up (keeping the task out of Recents). */
    @Test
    @Config(sdk = [28, 33])
    fun aMissingRecentsCallDoesNotSkipTheKeyGuard() = engageOnAMissingCall(MissingRecentsCallActivity::class.java)

    /** The call that shows the activity over the lock screen. */
    @Test
    @Config(sdk = [28, 33])
    fun aMissingLockScreenCallDoesNotSkipTheKeyGuard() = engageOnAMissingCall(MissingLockScreenCallActivity::class.java)

    class MissingRecentsCallActivity : Activity() {
        override fun getSystemService(name: String): Any? {
            if (name == ACTIVITY_SERVICE) throw NoSuchMethodError("ActivityManager.AppTask.setExcludeFromRecents")
            return super.getSystemService(name)
        }
    }

    class MissingLockScreenCallActivity : Activity() {
        override fun setShowWhenLocked(showWhenLocked: Boolean) {
            throw NoSuchMethodError("Activity.setShowWhenLocked")
        }
    }
}
