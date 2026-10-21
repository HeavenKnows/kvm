# kvm Android receiver — Ocean5 Pro / Android 14

This is the first buildable Android project for the 掌阅 Ocean5 Pro e-ink reader, targeting Android 14 / API 34 only (minSdk 34, compileSdk 34, targetSdk 34). It is deliberately not advertised as a working remote receiver yet.

Included: Android app module and dedicated APK CI; a user-enabled AccessibilityService; direct navigation to Android Accessibility Settings with permission state refreshed on return; tap/swipe helpers and focused text injection with local clipboard fallback; pairing-key format validation and unit tests.

Implemented in this protocol foundation: desktop-compatible Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s session setup, the exact lowercase-UTF-8 SHA-256 PSK derivation, two-byte big-endian length-prefixed encrypted frames, and a loopback encrypted round-trip test. A compact MessagePack adapter for the Rust Wire enum has been added, but it has not yet been interoperability-tested against the desktop peer. Listener/UI integration, remote input dispatch, e-ink refresh optimization, reconnect lifecycle, and one-way Windows/macOS-to-Android clipboard transport are still pending.

A successful APK build is not Android feature-complete status. The Noise framing foundation is tested locally in Android JVM tests, but it is not yet integrated into a listening receiver and has not yet been interoperability-tested against the Rust desktop peer.

Build with JDK 17, Android SDK Platform 34, and Gradle 8.9: gradle -p android testDebugUnitTest assembleDebug

The debug APK is produced at android/app/build/outputs/apk/debug/app-debug.apk. The dedicated workflow uploads the APK as an artifact after a successful run.
