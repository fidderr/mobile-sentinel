// :sentinel-media_picker — capability module. Compiled into the consumer build
// ONLY when the `media_picker` capability (Cargo feature) is enabled.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/media_picker for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/media_picker")
    else file("$projectDir/../../builds/media_picker")
)

android {
    namespace = "com.mobilesentinel.mediapicker"
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
