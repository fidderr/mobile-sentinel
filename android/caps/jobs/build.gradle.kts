// :sentinel-jobs — building-block module. Compiled into the consumer build
// ONLY when the `jobs` Cargo feature is enabled. Holds the cross-process job
// guardian (the `:sentinel` process that keeps MAIN alive / resurrects it) and
// the JOB_HEADS_UP forwarding receiver. Standalone: a consumer can enable
// `jobs` with no kiosk / firing surface at all.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/jobs (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/jobs"))

android {
    namespace = "com.mobilesentinel.jobs"
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
