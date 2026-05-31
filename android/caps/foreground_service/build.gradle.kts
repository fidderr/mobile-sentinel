// :sentinel-foreground_service — firing sub-feature module. Compiled into the
// consumer build ONLY when the `foreground-service` Cargo feature is enabled.
// Holds the alarm foreground service (keeps the process alive while firing)
// and its start/stop JNI surface.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/foreground_service (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/foreground_service"))

android {
    namespace = "com.mobilesentinel.foregroundservice"
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
