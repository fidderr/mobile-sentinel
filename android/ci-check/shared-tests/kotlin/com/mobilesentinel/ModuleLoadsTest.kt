package com.mobilesentinel

import android.content.Context
import android.content.pm.PackageManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/**
 * Compiled into the tests of every module (android/ci-check/build.gradle.kts), so a module without tests of its
 * own is still loaded on the Android 9 framework: every class of it links (its method, field and constructor types
 * exist there, which a class that names a newer framework class does not), and every component of its merged
 * manifest is a class that loads.
 */
@RunWith(RobolectricTestRunner::class)
class ModuleLoadsTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val loader: ClassLoader get() = javaClass.classLoader!!

    private fun moduleClasses(): List<String> {
        val root = File(System.getProperty("sentinel.classes") ?: error("sentinel.classes is not set"))
        return root.walkTopDown()
            .filter { it.isFile && it.extension == "class" }
            .map { it.relativeTo(root).path.removeSuffix(".class").replace(File.separatorChar, '.') }
            .sorted()
            .toList()
    }

    private fun linkingProblems(names: List<String>): List<String> = names.mapNotNull { name ->
        try {
            val c = Class.forName(name, false, loader)
            c.declaredMethods
            c.declaredFields
            c.declaredConstructors
            Class.forName(name, true, loader)
            null
        } catch (t: Throwable) {
            "$name: $t"
        }
    }

    @Test
    @Config(sdk = [28, 36])
    fun everyClassOfTheModuleLinks() {
        val names = moduleClasses()
        assertTrue("no compiled classes found", names.isNotEmpty())
        assertEquals(emptyList<String>(), linkingProblems(names))
    }

    @Test
    @Config(sdk = [28, 36])
    fun everyComponentOfTheMergedManifestIsAClassThatLoads() {
        val flags = PackageManager.GET_ACTIVITIES or PackageManager.GET_SERVICES or
            PackageManager.GET_RECEIVERS or PackageManager.GET_PROVIDERS or PackageManager.GET_DISABLED_COMPONENTS
        val info = context.packageManager.getPackageInfo(context.packageName, flags)
        val names = (info.activities.orEmpty().map { it.name } + info.services.orEmpty().map { it.name } +
            info.receivers.orEmpty().map { it.name } + info.providers.orEmpty().map { it.name })
        assertEquals(emptyList<String>(), linkingProblems(names))
    }
}
