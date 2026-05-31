// :sentinel-biometric — capability module. Compiled into the consumer build
// ONLY when the `biometric` capability (Cargo feature) is enabled.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// All generated output goes to android/builds/biometric (single
// gitignored location for every module). See android/.gitignore.
layout.buildDirectory.set(file("$projectDir/../../builds/biometric"))

android {
    namespace = "com.mobilesentinel.biometric"
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
    implementation("androidx.biometric:biometric:1.1.0")
    implementation("androidx.fragment:fragment-ktx:1.6.2")
}
