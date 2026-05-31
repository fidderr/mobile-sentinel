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

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/exact_alarm for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/exact_alarm")
    else file("$projectDir/../../builds/exact_alarm")
)

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
