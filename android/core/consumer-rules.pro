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

# Explicit for the known JNI entry points (defense in depth)
-keep class com.mobilesentinel.SentinelPrimitives { *; }
-keep class com.mobilesentinel.SentinelActivityTracker { *; }
-keep class com.mobilesentinel.SentinelCoreInitializer { *; }
-keep class com.mobilesentinel.SentinelPermissionPrimitives { *; }
-keep class com.mobilesentinel.SentinelOverlayPrimitives { *; }
-keep class com.mobilesentinel.SentinelScannerHelper { *; }
-keep class com.mobilesentinel.SentinelScannerActivity { *; }
-keep class com.mobilesentinel.SentinelKioskInitializer { *; }
-keep class com.mobilesentinel.SentinelJobsInitializer { *; }

# Also keep the ones in caps that may be instantiated by manifest
-keep class com.mobilesentinel.SentinelBootReceiver { *; }
-keep class com.mobilesentinel.SentinelAlarmReceiver { *; }
-keep class com.mobilesentinel.SentinelTimeChangeReceiver { *; }
-keep class com.mobilesentinel.SentinelKioskController { *; }
