// :sentinel-screen_pin — capability module. Compiled into the consumer build
// ONLY when the `screen_pin` capability (Cargo feature) is enabled.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/screen_pin (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/screen_pin"))

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
