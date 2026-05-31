// :sentinel-firing_vibration — firing sub-feature module. Compiled into the
// consumer build ONLY when the `firing-vibration` Cargo feature is enabled.
// Holds the looping alarm-vibration JNI surface used while an alarm fires.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/firing_vibration (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/firing_vibration"))

android {
    namespace = "com.mobilesentinel.firingvibration"
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
