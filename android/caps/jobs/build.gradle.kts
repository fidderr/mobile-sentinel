// :sentinel-jobs — building-block module. Compiled into the consumer build
// ONLY when the `jobs` Cargo feature is enabled. Holds the cross-process job
// guardian (the `:sentinel` process that keeps MAIN alive / resurrects it) and
// the JOB_HEADS_UP forwarding receiver. Standalone: a consumer can enable
// `jobs` with no kiosk / firing surface at all.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated output goes to a writable build root. The consumer build passes
// -PsentinelBuildRoot=<dir> (build_sentinel sets this) so output never lands in
// the read-only crate dir when mobile-sentinel is consumed from crates.io. Falls
// back to the in-tree android/builds/jobs for workspace/path-dependency dev.
val sentinelBuildRoot = project.findProperty("sentinelBuildRoot") as String?
layout.buildDirectory.set(
    if (sentinelBuildRoot != null) file("$sentinelBuildRoot/jobs")
    else file("$projectDir/../../builds/jobs")
)

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
