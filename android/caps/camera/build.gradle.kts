// :sentinel-camera — capability module. Compiled into the consumer build
// ONLY when the `camera` capability (Cargo feature) is enabled.
//
// Photo capture via the system camera app (ACTION_IMAGE_CAPTURE). No ZXing /
// CameraX dependency — that is the whole point of keeping this separate from
// the `scanner` capability.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/camera (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/camera"))

android {
    namespace = "com.mobilesentinel.camera"
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
