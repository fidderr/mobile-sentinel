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
