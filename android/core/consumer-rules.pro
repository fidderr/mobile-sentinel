# consumer-rules.pro for sentinel-core (and transitively its caps)
# Keeps the entire com.mobilesentinel package because:
# - Primitives objects (Sentinel*Primitives, Sentinel*Helper) are called exclusively
#   by JNI from Rust side using hardcoded class/method names.
# - Activities, receivers, providers, initializers are referenced from the
#   patched manifest and by name from other sentinel code.
# - R8/full minify (used in release AABs) would otherwise remove or rename them,
#   breaking permission requests, overlay settings launch, scanner, kiosk, firing, etc.
#
# IMPORTANT: This does NOT cause extra code to be included.
# Module selection (which caps' Kotlin is compiled at all) is done earlier by
# build_sentinel based strictly on the Cargo features listed in the app's
# Cargo.toml. Only selected modules appear in settings.gradle + dependencies,
# so Gradle only ever compiles the Kotlin for enabled features.
# This keep rule simply protects the (small) entry points inside whatever
# modules *were* selected.
#
# This file is declared via consumerProguardFiles in core/build.gradle.kts and
# gets merged into the final app's proguard rules when building the release AAB.

-keep class com.mobilesentinel.** { *; }
-keepclassmembers class com.mobilesentinel.** { *; }
