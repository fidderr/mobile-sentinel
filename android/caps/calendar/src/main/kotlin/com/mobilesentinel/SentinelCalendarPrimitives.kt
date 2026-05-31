package com.mobilesentinel

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.provider.CalendarContract
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.util.TimeZone

/**
 * Calendar platform primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around [CalendarContract] accessed through the application
 * [android.content.ContentResolver]. No orchestration logic, no state
 * machines — read / create / delete events and nothing more.
 *
 * These operations require the host app to hold the `READ_CALENDAR` /
 * `WRITE_CALENDAR` runtime permissions. When a permission isn't granted
 * the platform throws [SecurityException]; that is expected and each
 * method swallows it, logs a warning, and returns its safe fallback
 * (`"[]"` / `""` / `false`). No method ever throws.
 *
 * Context is obtained from [SentinelPrimitives.getAppContext]; when it is
 * null (init not yet called) every method returns its safe fallback.
 */
object SentinelCalendarPrimitives {
    private const val TAG = "MobileSentinel.Calendar"

    /** Primary calendar id used for inserts — an acceptable default. */
    private const val DEFAULT_CALENDAR_ID = 1L

    private val EVENT_PROJECTION = arrayOf(
        CalendarContract.Events._ID,
        CalendarContract.Events.TITLE,
        CalendarContract.Events.DESCRIPTION,
        CalendarContract.Events.DTSTART,
        CalendarContract.Events.DTEND,
        CalendarContract.Events.EVENT_LOCATION,
    )

    /**
     * Query events whose `DTSTART >= startMillis` and `DTEND <= endMillis`.
     *
     * Returns a JSON array string where each element is an object shaped:
     * `{"id":"...","title":"...","description":"...","start":<long>,"end":<long>,"location":"..."}`.
     * `description` / `location` are JSON `null` when the underlying column
     * is null. Returns `"[]"` on any failure or when context is unavailable.
     */
    @JvmStatic
    fun getEvents(startMillis: Long, endMillis: Long): String {
        val context: Context = SentinelPrimitives.getAppContext() ?: return "[]"
        return try {
            val selection =
                "${CalendarContract.Events.DTSTART} >= ? AND ${CalendarContract.Events.DTEND} <= ?"
            val selectionArgs = arrayOf(startMillis.toString(), endMillis.toString())
            val result = JSONArray()
            context.contentResolver.query(
                CalendarContract.Events.CONTENT_URI,
                EVENT_PROJECTION,
                selection,
                selectionArgs,
                "${CalendarContract.Events.DTSTART} ASC",
            )?.use { cursor ->
                val idIdx = cursor.getColumnIndexOrThrow(CalendarContract.Events._ID)
                val titleIdx = cursor.getColumnIndexOrThrow(CalendarContract.Events.TITLE)
                val descIdx = cursor.getColumnIndexOrThrow(CalendarContract.Events.DESCRIPTION)
                val startIdx = cursor.getColumnIndexOrThrow(CalendarContract.Events.DTSTART)
                val endIdx = cursor.getColumnIndexOrThrow(CalendarContract.Events.DTEND)
                val locIdx = cursor.getColumnIndexOrThrow(CalendarContract.Events.EVENT_LOCATION)
                while (cursor.moveToNext()) {
                    val obj = JSONObject()
                    obj.put("id", cursor.getLong(idIdx).toString())
                    obj.put("title", if (cursor.isNull(titleIdx)) "" else cursor.getString(titleIdx))
                    obj.put(
                        "description",
                        if (cursor.isNull(descIdx)) JSONObject.NULL else cursor.getString(descIdx),
                    )
                    obj.put("start", cursor.getLong(startIdx))
                    obj.put("end", cursor.getLong(endIdx))
                    obj.put(
                        "location",
                        if (cursor.isNull(locIdx)) JSONObject.NULL else cursor.getString(locIdx),
                    )
                    result.put(obj)
                }
            }
            result.toString()
        } catch (e: SecurityException) {
            Log.w(TAG, "getEvents: missing READ_CALENDAR permission: ${e.message}")
            "[]"
        } catch (e: Throwable) {
            Log.w(TAG, "getEvents: ${e.message}", e)
            "[]"
        }
    }

    /**
     * Insert an event into the primary calendar.
     *
     * Returns the created event id (the inserted Uri's last path segment),
     * or `""` on failure / when context is unavailable.
     */
    @JvmStatic
    fun createEvent(
        title: String,
        description: String?,
        startMillis: Long,
        endMillis: Long,
        location: String?,
    ): String {
        val context: Context = SentinelPrimitives.getAppContext() ?: return ""
        return try {
            val values = ContentValues().apply {
                put(CalendarContract.Events.TITLE, title)
                put(CalendarContract.Events.DESCRIPTION, description)
                put(CalendarContract.Events.DTSTART, startMillis)
                put(CalendarContract.Events.DTEND, endMillis)
                put(CalendarContract.Events.EVENT_LOCATION, location)
                put(CalendarContract.Events.CALENDAR_ID, DEFAULT_CALENDAR_ID)
                put(CalendarContract.Events.EVENT_TIMEZONE, TimeZone.getDefault().id)
            }
            val uri = context.contentResolver.insert(CalendarContract.Events.CONTENT_URI, values)
            uri?.lastPathSegment ?: ""
        } catch (e: SecurityException) {
            Log.w(TAG, "createEvent: missing WRITE_CALENDAR permission: ${e.message}")
            ""
        } catch (e: Throwable) {
            Log.w(TAG, "createEvent: ${e.message}", e)
            ""
        }
    }

    /**
     * Delete an event by id.
     *
     * Returns `true` when at least one row was deleted, `false` on failure,
     * when nothing matched, or when context is unavailable.
     */
    @JvmStatic
    fun deleteEvent(id: String): Boolean {
        val context: Context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val eventId = id.toLong()
            val uri = ContentUris.withAppendedId(CalendarContract.Events.CONTENT_URI, eventId)
            val deleted = context.contentResolver.delete(uri, null, null)
            deleted > 0
        } catch (e: SecurityException) {
            Log.w(TAG, "deleteEvent($id): missing WRITE_CALENDAR permission: ${e.message}")
            false
        } catch (e: NumberFormatException) {
            Log.w(TAG, "deleteEvent($id): id is not a valid long: ${e.message}")
            false
        } catch (e: Throwable) {
            Log.w(TAG, "deleteEvent($id): ${e.message}", e)
            false
        }
    }
}
