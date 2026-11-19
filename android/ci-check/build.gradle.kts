// The versions the Dioxus-generated consumer project uses.
buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.7.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.0.20")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

// `lintDebug -PsentinelMinSdk=26` fails on a call newer than that minimum without a guard.
subprojects {
    plugins.withId("com.android.library") {
        extensions.configure<com.android.build.api.dsl.LibraryExtension>("android") {
            lint {
                checkOnly += "NewApi"
                error += "NewApi"
                abortOnError = true
                warningsAsErrors = false
            }
        }
    }
}

// JVM unit tests (Robolectric) of the Kotlin modules, run per module from its src/test:
//
//   gradle -p android/ci-check testDebugUnitTest -PsentinelMinSdk=26
//
// The test dependencies live here, not in the modules, so a consumer's build never sees them.
// Robolectric needs Java 21 to run Android 16 (API 36), so the tests run on a JDK 21 launcher
// (Gradle finds an installed one) while Gradle itself and the compilation stay on 17.
subprojects {
    plugins.withId("com.android.library") {
        extensions.configure<com.android.build.api.dsl.LibraryExtension>("android") {
            testOptions.unitTests.isIncludeAndroidResources = true
        }
        dependencies {
            add("testImplementation", "junit:junit:4.13.2")
            add("testImplementation", "org.robolectric:robolectric:4.18.1")
            add("testImplementation", "androidx.test:core:1.6.1")
        }
        val launchers = extensions.getByType<JavaToolchainService>()
        tasks.withType<Test>().configureEach {
            javaLauncher.set(launchers.launcherFor { languageVersion.set(JavaLanguageVersion.of(21)) })
            maxHeapSize = "1g"
            failOnNoDiscoveredTests = false
            // The ring plans Rust writes (tests/fixtures), which the Kotlin tests read too.
            systemProperty("sentinel.fixtures", file("$rootDir/../../tests/fixtures").path)
            testLogging {
                events("failed", "skipped")
                exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
            }
        }
    }
}
