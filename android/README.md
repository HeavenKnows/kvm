# kvm Android receiver — Ocean5 Pro / Android 14

This is the first buildable Android project for the 掌阅 Ocean5 Pro e-ink reader, targeting Android 14 / API 34 only (minSdk 34, compileSdk 34, targetSdk 34). It is deliberately not advertised as a working remote receiver yet.

Included: Android app module and dedicated APK CI; a user-enabled AccessibilityService; direct navigation to Android Accessibility Settings with permission state refreshed on return; tap/swipe helpers and focused text injection with local clipboard fallback; pairing-key format validation and unit tests.

Not implemented: the desktop-compatible Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s handshake, encrypted TCP, MessagePack compatibility with src/wire.rs, remote mouse/keyboard handling, e-ink refresh optimization, reconnect lifecycle, or one-way Windows/macOS-to-Android clipboard transport.

A successful APK build is only a buildable scaffold, not Android feature-complete status. The protocol adapter must be tested against the existing Rust implementation before remote input or clipboard data is connected.

Build with JDK 17, Android SDK Platform 34, and Gradle 8.9: gradle -p android testDebugUnitTest assembleDebug

The debug APK is produced at android/app/build/outputs/apk/debug/app-debug.apk. The dedicated workflow uploads the APK as an artifact after a successful run.
