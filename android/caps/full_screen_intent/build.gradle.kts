// :sentinel-full_screen_intent — firing sub-feature module. Compiled into the
// consumer build ONLY when the `full-screen-intent` Cargo feature is enabled.
// Holds the JNI to launch the firing activity over the lock screen.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/full_screen_intent (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/full_screen_intent"))

android {
    namespace = "com.mobilesentinel.fullscreenintent"
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
