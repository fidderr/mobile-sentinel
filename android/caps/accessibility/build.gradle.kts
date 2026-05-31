// :sentinel-accessibility — capability module. Compiled into the consumer build
// ONLY when the `accessibility` capability (Cargo feature) is enabled.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/accessibility (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/accessibility"))

android {
    namespace = "com.mobilesentinel.accessibility"
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
    // Accessibility is the kiosk ultra-protection enforcer — it reads
    // SentinelKioskController, which lives in the kiosk module.
    implementation(project(":sentinel-kiosk"))
    implementation("androidx.core:core-ktx:1.12.0")
}
