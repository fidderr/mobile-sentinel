// :sentinel-firing_audio — firing sub-feature module. Compiled into the
// consumer build ONLY when the `firing-audio` Cargo feature is enabled. Holds
// the alarm audio playback JNI surface used while an alarm fires.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/firing_audio (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/firing_audio"))

android {
    namespace = "com.mobilesentinel.firingaudio"
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
