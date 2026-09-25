plugins {
    id("com.android.application")
}

android {
    namespace = "com.goldtone.duckfoot"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.goldtone.duckfoot"
        // 30 (Android 11) is where an app may write images into the shared Pictures/
        // directory by path without a permission, which is how exports reach the gallery.
        minSdk = 30
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            // The Rust library is built for these by build-apk.sh.
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Signed with the debug key until there is a release keystore; installable,
            // not publishable.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
