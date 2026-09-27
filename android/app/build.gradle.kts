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
        // CI numbers builds so each release installs over the one before; a version tag
        // names it (see .github/workflows/build.yml).
        versionCode = System.getenv("GITHUB_RUN_NUMBER")?.toIntOrNull() ?: 1
        versionName = System.getenv("DUCKFOOT_VERSION_NAME")?.takeIf { it.isNotEmpty() } ?: "0.1.0"
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
