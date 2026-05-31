// :sentinel-kiosk — firing sub-feature module. Compiled into the consumer
// build ONLY when the `kiosk` Cargo feature is enabled. Holds the activity-
// lock controller (keeps the firing activity in the foreground, relaunches it
// on HOME/Recents, consumes Back), its cross-process state file, and the
// enable/disable JNI surface.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/kiosk (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/kiosk"))

android {
    namespace = "com.mobilesentinel.kiosk"
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
