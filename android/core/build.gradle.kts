// :sentinel-core — always-present mobile-sentinel kernel.
//
// This module holds ONLY the irreducible kernel every consumer carries: the
// JNI context holder (SentinelPrimitives = init + getAppContext), the native
// bridge (SentinelBridge), the resumed-activity tracker
// (SentinelActivityTracker), and the library-init provider
// (SentinelCoreInitializer). It references NO capability module — capability
// modules depend on core, never the reverse. Its baseline manifest declares a
// single init <provider> and ZERO permissions.
//
// The firing surfaces (foreground service, alarm/boot/time receivers, boot
// service, job guardian, kiosk controller) each live in their OWN per-feature
// module (../caps/<id>: foreground_service, exact_alarm, firing_audio,
// wake_lock, kiosk, full_screen_intent, jobs), pulled in only when that
// capability is enabled. Every leaf capability lives in its own
// `:sentinel-<cap>` module (../caps/<cap>).
// build_sentinel includes core + only the ENABLED capability modules, so a
// disabled capability's Kotlin/deps are never compiled (structural trimming —
// no source exclusion).
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/core for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/core")
    else file("$projectDir/../builds/core")
)

android {
    namespace = "com.mobilesentinel"
    compileSdk = 34

    defaultConfig {
        minSdk = 24
        targetSdk = 34
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    testOptions {
        unitTests.isIncludeAndroidResources = false
        unitTests.isReturnDefaultValues = true
    }
}

dependencies {
    // Core Kotlin uses NotificationCompat etc.
    implementation("androidx.core:core-ktx:1.12.0")

    // JUnit 4 for JVM unit tests under src/test/kotlin.
    testImplementation("junit:junit:4.13.2")
}
