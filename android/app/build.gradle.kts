plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}
android {
    namespace = "dev.heavenknows.kvmreader"
    compileSdk = 34
    defaultConfig {
        applicationId = "dev.heavenknows.kvmreader"
        minSdk = 34
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0-dev"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}
dependencies {
    implementation("com.github.auties00:noise-java:1.2")
    implementation("org.msgpack:msgpack-core:0.9.8")
    testImplementation("junit:junit:4.13.2")
}
