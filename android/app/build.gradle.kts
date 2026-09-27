plugins {
    id("com.android.application")
}

android {
    namespace = "com.walker.duckfoot"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.walker.duckfoot"
        // 30 (Android 11) is where an app may write images into the shared Pictures/
        // directory by path without a permission, which is how exports reach the gallery.
        minSdk = 30
        targetSdk = 35
        // A version tag names the build (see .github/workflows/build.yml), and its code
        // is the same scheme everywhere (L-17): major.minor.patch, then CI's run number
        // (0 for a local build), so a newer version always installs over an older one and,
        // within a version, each CI build over the one before.
        versionName = System.getenv("DUCKFOOT_VERSION_NAME")?.takeIf { it.isNotEmpty() } ?: "0.1.0"
        val parts = (versionName ?: "0.1.0").split(".").map { part ->
            part.filter { c -> c.isDigit() }.toIntOrNull() ?: 0
        }
        val major = parts.getOrElse(0) { 0 }
        val minor = parts.getOrElse(1) { 0 }
        val patch = parts.getOrElse(2) { 0 }
        val run = (System.getenv("GITHUB_RUN_NUMBER")?.toIntOrNull() ?: 0) % 10000
        versionCode = (major * 10000 + minor * 100 + patch) * 10000 + run
        ndk {
            // The Rust library is built for these by build-apk.sh.
            abiFilters += listOf("arm64-v8a")
        }
    }

    // Android installs an update only over an app signed with the same key. Given a
    // keystore (android/README.md), every build signs with it. Without one, builds sign
    // with the testing key checked in beside this file, so every build - CI's on any
    // branch, or a local one - installs over the last without uninstalling first. It is
    // a key for testing builds only; anything published gets the repository's own.
    val keystore = System.getenv("DUCKFOOT_KEYSTORE")
    signingConfigs.getByName("debug") {
        if (keystore != null) {
            storeFile = file(keystore)
            storePassword = System.getenv("DUCKFOOT_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("DUCKFOOT_KEY_ALIAS")
            keyPassword = System.getenv("DUCKFOOT_KEY_PASSWORD")
        } else {
            storeFile = rootProject.file("testing.jks")
            storePassword = "duckfoot-testing"
            keyAlias = "duckfoot"
            keyPassword = "duckfoot-testing"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // The same key as debug builds (above): installable, not yet publishable.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
