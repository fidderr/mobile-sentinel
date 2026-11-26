package com.mobilesentinel

import android.app.Application
import android.app.NotificationManager
import android.content.Context
import android.provider.Settings
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** POST_NOTIFICATIONS is a runtime permission from Android 13; before that the notification switch is the truth. */
@RunWith(RobolectricTestRunner::class)
class SentinelPermissionPrimitivesTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val app get() = context as Application
    private val post = "android.permission.POST_NOTIFICATIONS"

    @Before
    fun setUp() {
        SentinelPrimitives.init(context)
    }

    private fun notificationsSwitch(on: Boolean) =
        shadowOf(context.getSystemService(NotificationManager::class.java)).setNotificationsEnabled(on)

    @Test
    @Config(sdk = [26, 28, 30, 31, 32])
    fun below13PostNotificationsFollowsTheNotificationSwitch() {
        notificationsSwitch(true)
        assertTrue(SentinelPermissionPrimitives.checkPermission(post))
        notificationsSwitch(false)
        assertFalse(SentinelPermissionPrimitives.checkPermission(post))
    }

    /** From 13 the switch is irrelevant to the permission: only the grant counts. */
    @Test
    @Config(sdk = [33, 34, 36])
    fun from13PostNotificationsFollowsTheGrant() {
        notificationsSwitch(true)
        assertFalse(SentinelPermissionPrimitives.checkPermission(post))
        shadowOf(app).grantPermissions(post)
        assertTrue(SentinelPermissionPrimitives.checkPermission(post))
    }

    @Test
    @Config(sdk = [26, 28, 30, 32])
    fun below13RequestingWhenOnAsksNothing() {
        notificationsSwitch(true)
        assertEquals(0, SentinelPermissionPrimitives.requestRuntimePermission(post))
        assertNull(shadowOf(app).nextStartedActivity)
    }

    /** There is no dialog to show below 13: the app's notification settings open instead. */
    @Test
    @Config(sdk = [26, 28, 30, 32])
    fun below13RequestingWhenOffOpensTheNotificationSettings() {
        notificationsSwitch(false)
        assertEquals(1, SentinelPermissionPrimitives.requestRuntimePermission(post))
        val intent = shadowOf(app).nextStartedActivity
        assertEquals(Settings.ACTION_APP_NOTIFICATION_SETTINGS, intent.action)
        assertEquals(context.packageName, intent.getStringExtra(Settings.EXTRA_APP_PACKAGE))
    }

    @Test
    @Config(sdk = [33, 36])
    fun from13RequestingWhenGrantedReturnsZero() {
        shadowOf(app).grantPermissions(post)
        assertEquals(0, SentinelPermissionPrimitives.requestRuntimePermission(post))
    }

    /** From 13 without a screen to host the dialog nothing opens: not granted, and no settings page. */
    @Test
    @Config(sdk = [33, 36])
    fun from13RequestingWithoutAScreenIsNotGrantedAndOpensNothing() {
        assertEquals(1, SentinelPermissionPrimitives.requestRuntimePermission(post))
        assertNull(shadowOf(app).nextStartedActivity)
    }

    @Test
    @Config(sdk = [28, 33])
    fun otherPermissionsUseTheGrant() {
        val camera = "android.permission.CAMERA"
        assertFalse(SentinelPermissionPrimitives.checkPermission(camera))
        shadowOf(app).grantPermissions(camera)
        assertTrue(SentinelPermissionPrimitives.checkPermission(camera))
    }
}
