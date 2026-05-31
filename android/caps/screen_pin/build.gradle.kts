// :sentinel-screen_pin — capability module. Compiled into the consumer build
// ONLY when the `screen_pin` capability (Cargo feature) is enabled.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/screen_pin for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/screen_pin")
    else file("$projectDir/../../builds/screen_pin")
)

android {
    namespace = "com.mobilesentinel.screenpin"
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
    implementation("androidx.core:core-ktx:1.12.0")
}
