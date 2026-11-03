package com.mobilesentinel

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.os.Build
import android.util.Log
import android.widget.RemoteViews
import com.mobilesentinel.home_widget.R
import java.io.File

/**
 * A home-screen widget with two lines the app decides on. Tapping it opens the app.
 *
 * Rust sets the text whenever it changes (`home_widget::set_text`). The file keeps it
 * for when Android redraws widgets after a restart, with the app not running.
 */
class SentinelHomeWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        SentinelHomeWidgetPrimitives.draw(context, manager, ids)
    }
}

object SentinelHomeWidgetPrimitives {
    private const val TAG = "MobileSentinel.Widget"

    private fun file(context: Context) = File(SentinelStorage.filesDir(context), "sentinel/home_widget.txt")

    internal fun draw(context: Context, manager: AppWidgetManager, ids: IntArray) {
        try {
            val saved = file(context).takeIf { it.exists() }?.readLines().orEmpty()
            val views = RemoteViews(context.packageName, R.layout.sentinel_home_widget)
            // Only before the app ran for the first time.
            val title = saved.getOrNull(0) ?: context.applicationInfo.loadLabel(context.packageManager)
            views.setTextViewText(R.id.sentinel_widget_title, title)
            views.setTextViewText(R.id.sentinel_widget_detail, saved.getOrNull(1).orEmpty())
            context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { open ->
                views.setOnClickPendingIntent(
                    R.id.sentinel_widget_root,
                    PendingIntent.getActivity(
                        context,
                        0,
                        open,
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            }
            manager.updateAppWidget(ids, views)
        } catch (e: Throwable) {
            Log.w(TAG, "drawing the widget failed: ${e.message}")
        }
    }

    @JvmStatic
    fun setText(title: String, detail: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val target = file(context)
            val text = "$title\n$detail"
            // This runs on every change to the alarms: nothing to do most of the time.
            if (target.exists() && target.readText() == text) return true
            target.parentFile?.mkdirs()
            val written = File(target.path + ".tmp")
            written.writeText(text)
            written.renameTo(target)
            val manager = AppWidgetManager.getInstance(context) ?: return true
            val ids = manager.getAppWidgetIds(ComponentName(context, SentinelHomeWidget::class.java))
            if (ids.isNotEmpty()) draw(context, manager, ids)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setting the widget text failed: ${e.message}")
            false
        }
    }

    /** Whether the launcher can be asked to place the widget. */
    @JvmStatic
    fun canAdd(): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return false
        return try {
            AppWidgetManager.getInstance(context)?.isRequestPinAppWidgetSupported == true
        } catch (e: Throwable) {
            false
        }
    }

    /** Ask the launcher to place the widget. It asks the user first. */
    @JvmStatic
    fun add(): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return false
        return try {
            AppWidgetManager.getInstance(context)
                ?.requestPinAppWidget(ComponentName(context, SentinelHomeWidget::class.java), null, null) == true
        } catch (e: Throwable) {
            Log.w(TAG, "adding the widget failed: ${e.message}")
            false
        }
    }
}
