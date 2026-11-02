// Compiles every Kotlin module on its own. Consumers only ever build the modules
// they enable, so without this a module none of them uses can break unnoticed.
//
//   gradle -p android/ci-check compileDebugKotlin
rootProject.name = "sentinel-ci-check"

include(":sentinel-core")
project(":sentinel-core").projectDir = file("../core")

file("../caps").listFiles()!!
    .filter { File(it, "build.gradle.kts").exists() }
    .forEach {
        include(":sentinel-${it.name}")
        project(":sentinel-${it.name}").projectDir = it
    }
