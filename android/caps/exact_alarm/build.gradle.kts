// :sentinel-exact_alarm — firing sub-feature module. Compiled into the
// consumer build ONLY when the `exact-alarm` Cargo feature is enabled. Owns
// OS exact-alarm scheduling (AlarmManager) plus the boot / time-change
// receivers that re-arm scheduled alarms after a reboot or clock change.
//
// Depends on :sentinel-jobs: a scheduled OS wake activates a job and starts
// the guardian (the one honest edge — scheduling needs the job system to
// resurrect MAIN at fire time).
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/exact_alarm (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/exact_alarm"))

android {
    namespace = "com.mobilesentinel.exactalarm"
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
    implementation(project(":sentinel-jobs"))
    implementation("androidx.core:core-ktx:1.12.0")
}
