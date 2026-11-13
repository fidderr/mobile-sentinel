package com.mobilesentinel.rust

import android.app.Activity
import android.app.NativeActivity
import android.app.Service
import android.app.admin.DeviceAdminReceiver
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Binder
import android.os.Bundle
import android.os.IBinder

/**
 * Android components whose code is Rust.
 *
 * Android creates services, receivers and activities by class name, so each needs a
 * class. build_sentinel generates a one-line subclass of one of these for every component
 * of the app's manifest that no source defines; it implements [rust] by calling the
 * bridge of the native library that handles the component (`Lib_<name>.event`). Every
 * lifecycle event becomes one call: `rust(event, self, intent, a, b)`.
 *
 * Events and what Rust returns:
 * - service: "create"; "start" (a = flags, b = startId; an Integer start mode, null for
 *   START_NOT_STICKY); "bind" (an IBinder or null); "unbind" (a Boolean, rebind wanted);
 *   "destroy".
 * - receiver: "receive" (self is the Context). Admin receiver also: "enabled",
 *   "provisioned" (profile provisioning complete).
 * - activity: "create", "new_intent", "resume", "pause", "destroy",
 *   "result" (a = requestCode, b = resultCode, intent = the data).
 * - native activity (a screen the library draws, that also wants these): "new_intent",
 *   "result".
 */
interface RustComponent {
    fun rust(event: String, self: Any, intent: Intent?, a: Int, b: Int): Any?
}

abstract class RustService : Service(), RustComponent {
    override fun onCreate() {
        super.onCreate()
        rust("create", this, null, 0, 0)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int =
        (rust("start", this, intent, flags, startId) as? Int) ?: START_NOT_STICKY

    override fun onBind(intent: Intent): IBinder? = rust("bind", this, intent, 0, 0) as? IBinder

    override fun onUnbind(intent: Intent): Boolean = (rust("unbind", this, intent, 0, 0) as? Boolean) ?: false

    override fun onDestroy() {
        rust("destroy", this, null, 0, 0)
        super.onDestroy()
    }
}

abstract class RustReceiver : BroadcastReceiver(), RustComponent {
    override fun onReceive(context: Context, intent: Intent) {
        rust("receive", context, intent, 0, 0)
    }
}

abstract class RustAdminReceiver : DeviceAdminReceiver(), RustComponent {
    override fun onEnabled(context: Context, intent: Intent) {
        rust("enabled", context, intent, 0, 0)
    }

    override fun onProfileProvisioningComplete(context: Context, intent: Intent) {
        rust("provisioned", context, intent, 0, 0)
    }
}

abstract class RustActivity : Activity(), RustComponent {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        rust("create", this, intent, 0, 0)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        rust("new_intent", this, intent, 0, 0)
    }

    override fun onResume() {
        super.onResume()
        rust("resume", this, intent, 0, 0)
    }

    override fun onPause() {
        rust("pause", this, intent, 0, 0)
        super.onPause()
    }

    override fun onDestroy() {
        rust("destroy", this, null, 0, 0)
        super.onDestroy()
    }

    @Deprecated("Activity result API of the platform")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        rust("result", this, data, requestCode, resultCode)
    }
}

abstract class RustNativeActivity : NativeActivity(), RustComponent {
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        rust("new_intent", this, intent, 0, 0)
    }

    @Deprecated("Activity result API of the platform")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        rust("result", this, data, requestCode, resultCode)
    }
}

/**
 * A receiver Rust registers at run time (for broadcasts only such a receiver gets, like
 * screen off). Its broadcasts arrive at `Lib_<lib>.event(name, "receive", context, intent)`.
 */
class RustRuntimeReceiver(private val lib: String, private val name: String) : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        try {
            Class.forName("com.mobilesentinel.rust.Lib_$lib")
                .getMethod(
                    "event",
                    String::class.java,
                    String::class.java,
                    Any::class.java,
                    Any::class.java,
                    Int::class.javaPrimitiveType,
                    Int::class.javaPrimitiveType,
                )
                .invoke(null, name, "receive", context, intent, 0, 0)
        } catch (e: ReflectiveOperationException) {
            android.util.Log.w("RustRuntimeReceiver", "no bridge for lib$lib: $e")
        }
    }
}

/**
 * A service connection Rust holds: Rust binds with it and lets go with it. It reports
 * nothing back; the bound service's process stays up while it is bound.
 */
class RustConnection : ServiceConnection {
    @Volatile var connected: Boolean = false
        private set

    override fun onServiceConnected(name: ComponentName, service: IBinder) {
        connected = true
    }

    override fun onServiceDisconnected(name: ComponentName) {
        connected = false
    }
}

/** A plain Binder for a service that only needs to be bound to stay up. */
object RustBinders {
    @JvmStatic fun plain(): IBinder = Binder()
}
