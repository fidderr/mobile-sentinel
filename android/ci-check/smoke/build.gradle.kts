// The device smoke test: the capabilities that ring an alarm, packed with one instrumentation test (src/androidTest) that
// runs on a real Android runtime in the SDK's own CI (.github/workflows/ci.yml, job "device smoke"). Not shipped: ci-check
// is excluded from the crate. Run it on a connected device or emulator with `bash android/ci-check/smoke/ci/run.sh`.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.mobilesentinel.smoke"
    compileSdk = 34
    defaultConfig {
        minSdk = (project.findProperty("sentinelMinSdk") as String?)?.toInt() ?: 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    testOptions { targetSdk = 34 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    for (cap in listOf(
        "core", "ring", "jobs", "exact_alarm", "foreground_service", "firing_audio", "firing_vibration",
        "audio", "share", "kiosk",
    )) implementation(project(":sentinel-$cap"))
    androidTestImplementation("androidx.test:runner:1.6.2")
}
