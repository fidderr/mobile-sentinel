// :sentinel-accessibility — capability module. Compiled into the consumer build
// ONLY when the `accessibility` capability (Cargo feature) is enabled.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/accessibility for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/accessibility")
    else file("$projectDir/../../builds/accessibility")
)

android {
    namespace = "com.mobilesentinel.accessibility"
    compileSdk = 34
    defaultConfig { minSdk = 24 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    implementation(project(":sentinel-core"))
    // Accessibility is the kiosk ultra-protection enforcer — it reads
    // SentinelKioskController, which lives in the kiosk module.
    implementation(project(":sentinel-kiosk"))
    implementation("androidx.core:core-ktx:1.12.0")
}
