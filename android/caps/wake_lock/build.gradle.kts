// :sentinel-wake_lock — firing sub-feature module. Compiled into the consumer
// build ONLY when the `wake-lock` Cargo feature is enabled. Holds the CPU
// wake-lock JNI surface used while an alarm fires.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/wake_lock (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/wake_lock"))

android {
    namespace = "com.mobilesentinel.wakelock"
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
