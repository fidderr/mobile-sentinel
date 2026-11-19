package com.mobilesentinel

import android.app.AlarmManager
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowAlarmManager

/** What the exact-alarm tests share: an initialised SDK, the alarm manager, and a job file. */
internal object ExactAlarmTestSupport {
    val context: Context get() = ApplicationProvider.getApplicationContext()

    val alarmManager: AlarmManager get() = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    fun scheduled(): List<ShadowAlarmManager.ScheduledAlarm> = shadowOf(alarmManager).scheduledAlarms

    /** Mirror of what Rust writes when it saves a job, which makes an alarm's job "exist". */
    fun writeJobFile(id: String) {
        val dir = java.io.File(SentinelStorage.filesDir(context), "sentinel/jobs")
        dir.mkdirs()
        java.io.File(dir, "$id.json").writeText(JSONObject().put("id", id).toString())
    }

    /** The AlarmManager forgets everything at a reboot; the armed record on disk does not. */
    fun forgetAllOsAlarms() {
        for (a in scheduled().toList()) alarmManager.cancel(a.operation!!)
    }
}
