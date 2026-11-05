// :sentinel-background_service — capability module. Compiled into the consumer
// build ONLY when the `background_service` capability (Cargo feature) is enabled.
//
// A quiet foreground service that keeps the app's process (and so its network
// connection) alive after the UI is closed, a boot receiver that brings it
// back, message/call notifications that open the right screen when tapped,
// and a Back-button hook so the app goes to the background instead of dying.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/background_service for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/background_service")
    else file("$projectDir/../../builds/background_service")
)

android {
    namespace = "com.mobilesentinel.backgroundservice"
    compileSdk = 34
    defaultConfig { minSdk = 31 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    implementation(project(":sentinel-core"))
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.activity:activity:1.8.2")
}
