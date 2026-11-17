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
        versionName = System.getenv("DUCKFOOT_VERSION_NAME")?.takeIf { it.isNotEmpty() } ?: "0.3.0"
        val parts = (versionName ?: "0.3.0").split(".").map { part ->
            part.filter { c -> c.isDigit() }.toIntOrNull() ?: 0
        }
        val major = parts.getOrElse(0) { 0 }
        val minor = parts.getOrElse(1) { 0 }
        val patch = parts.getOrElse(2) { 0 }
        val run = (System.getenv("GITHUB_RUN_NUMBER")?.toIntOrNull() ?: 0) % 1000
        // 100,000,000 + (major·100 + minor)·1000 + patch, then ·1000 + the run: room for
        // patch releases up to 999 and minor up to 99 without one version's codes landing
        // on another's (the old ·100 scheme put 0.3.100 on 0.4.0), and above every code
        // the old scheme gave (0.3.23 was 3,230,xxx).
        versionCode = 100_000_000 + ((major * 100 + minor) * 1000 + patch) * 1000 + run
        manifestPlaceholders["appLabel"] = "Duckfoot"
        ndk {
            // The Rust library is built for these by build-apk.sh.
            abiFilters += listOf("arm64-v8a")
        }
    }

    // Android installs an update only over an app signed with the same key, and lets it
    // read the app's data. The testing key beside this file is public (its password is
    // here), so nothing signed with it may ever be com.walker.duckfoot: anyone could sign
    // an "update" that installs over it and reads the user's photos and edits.
    //
    // So there are two apps. A build signed with the repository's own key
    // (DUCKFOOT_KEYSTORE, see android/README.md) is com.walker.duckfoot. Every other
    // build (debug, a local release without the key, CI's branch builds with
    // DUCKFOOT_TESTING_BUILD set) is com.walker.duckfoot.testing, signed with the testing
    // key, labelled "Duckfoot testing", and installs beside the real app, never over it.
    val keystore = System.getenv("DUCKFOOT_KEYSTORE")?.takeIf { it.isNotEmpty() }
    val testingBuild = System.getenv("DUCKFOOT_TESTING_BUILD") == "1"
    signingConfigs {
        create("testing") {
            storeFile = rootProject.file("testing.jks")
            storePassword = "duckfoot-testing"
            keyAlias = "duckfoot"
            keyPassword = "duckfoot-testing"
        }
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("DUCKFOOT_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("DUCKFOOT_KEY_ALIAS")
                keyPassword = System.getenv("DUCKFOOT_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".testing"
            versionNameSuffix = "-testing"
            signingConfig = signingConfigs.getByName("testing")
            manifestPlaceholders["appLabel"] = "Duckfoot testing"
        }
        release {
            isMinifyEnabled = false
            if (keystore != null && !testingBuild) {
                signingConfig = signingConfigs.getByName("release")
            } else {
                applicationIdSuffix = ".testing"
                versionNameSuffix = "-testing"
                signingConfig = signingConfigs.getByName("testing")
                manifestPlaceholders["appLabel"] = "Duckfoot testing"
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
