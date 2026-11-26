// Compiles every Kotlin module on its own. Consumers only ever build the modules
// they enable, so without this a module none of them uses can break unnoticed.
//
//   gradle -p android/ci-check compileDebugKotlin
//   gradle -p android/ci-check lintDebug -PsentinelMinSdk=26   (NewApi is an error: every call newer than 26 needs a guard)
//   gradle -p android/ci-check testDebugUnitTest -PsentinelMinSdk=26   (Robolectric tests of the modules, in their src/test)
//   gradle -p android/ci-check :sentinel-smoke:assembleDebugAndroidTest -PsentinelMinSdk=26   (the device smoke test, run by smoke/ci/run.sh)
rootProject.name = "sentinel-ci-check"

include(":sentinel-core")
project(":sentinel-core").projectDir = file("../core")

file("../caps").listFiles()!!
    .filter { File(it, "build.gradle.kts").exists() }
    .forEach {
        include(":sentinel-${it.name}")
        project(":sentinel-${it.name}").projectDir = it
    }

// The instrumented smoke test (runs on a device, in the CI job "device smoke").
include(":sentinel-smoke")
project(":sentinel-smoke").projectDir = file("smoke")
