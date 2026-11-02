package com.mobilesentinel

import android.content.Context
import android.media.RingtoneManager
import android.util.Log

/**
 * Core kernel context holder.
 *
 * This is the irreducible always-on surface every consumer carries: it stores
 * the application context during init and hands it out to capability modules.
 * It contains NO capability logic — notifications, permissions, overlay,
 * file-system, foregrounding, audio/wake-lock/kiosk/firing, etc. each live in
 * their own `:sentinel-<cap>` Gradle module and are compiled only when that
 * capability's Cargo feature is enabled. A no-feature consumer ships only
 * this kernel.
 *
 * Threading: every method is `@JvmStatic` and stateless apart from the static
 * `appContext` set during init. JNI callers attach their own thread.
 */
object SentinelPrimitives {
    private const val TAG = "MobileSentinel.Prim"

    @JvmStatic
    private var appContext: Context? = null

    /**
     * Must be called once from `Application.onCreate` (via the core
     * library-init provider). Stores the application context for later
     * capability calls.
     */
    @JvmStatic
    fun init(ctx: Context) {
        appContext = ctx.applicationContext
        Log.i(TAG, "SentinelPrimitives initialised for ${ctx.applicationContext.packageName}")
    }

    /**
     * The application context, or `null` before [init]. Used by every
     * capability module (which depend on `:sentinel-core`) to reach Android
     * system services without an Activity reference.
     */
    @JvmStatic
    fun getAppContext(): Context? = appContext

    /** The device's language as a two-letter code ("nl"). */
    @JvmStatic
    fun getDeviceLanguage(): String = java.util.Locale.getDefault().language

    /** The app's files directory, for native code that may run without an activity. Empty before [init]. */
    @JvmStatic
    fun getFilesDirPath(): String = appContext?.let { SentinelStorage.filesDir(it).absolutePath }.orEmpty()

    /**
     * IANA time-zone id (e.g. "Europe/Amsterdam"). A generic device query
     * (no alarm/firing semantics) used by the recipe layer to resolve
     * schedules in the user's local zone. Lives in core so consumers can read
     * it without enabling any firing surface. Empty when it cannot be read,
     * never a made-up zone: that would move alarms by hours.
     */
    @JvmStatic
    fun getTimeZoneId(): String {
        return try {
            java.util.TimeZone.getDefault().id.orEmpty()
        } catch (e: Throwable) {
            Log.w(TAG, "getTimeZoneId: ${e.message}")
            ""
        }
    }

    /**
     * The platform's default alarm ringtone URI, or a sentinel fallback. A
     * generic device query used by the Sound Library; lives in core so the
     * sound-library feature does not depend on any firing module.
     */
    @JvmStatic
    fun getSystemDefaultSoundUri(): String {
        return RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)?.toString()
            ?: "system://default-alarm"
    }
}
