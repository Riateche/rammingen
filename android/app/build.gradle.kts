plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.ktlint)
}

android {
    namespace = "me.darkecho.rammingen"
    compileSdk {
        version = release(36)
    }

    defaultConfig {
        applicationId = "me.darkecho.rammingen"
        minSdk = 21
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    kotlinOptions {
        jvmTarget = "11"
    }
    lint {
        warningsAsErrors = true
        disable.addAll(arrayOf("AndroidGradlePluginVersion", "GradleDependency"))
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }

    val buildNativeLibrary =
        tasks.register<Exec>("buildNativeLibrary") {
            println(">>> Building native library")
            workingDir = rootDir
            commandLine("native/build.sh")
        }

    // Ensure it runs before every build
    tasks.named("preBuild") {
        dependsOn(buildNativeLibrary)
    }

    tasks.build {
        dependsOn("buildNativeLibrary")
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.foundation)
    implementation(libs.androidx.material3)
    implementation(libs.androidx.documentfile)
    implementation(libs.androidx.preference.ktx)
    implementation(libs.google.tink.android)
    implementation(libs.androidx.appcompat)
    implementation(libs.androidx.preference)
    implementation(libs.material)
    implementation(libs.androidx.compose.material.icons.extended)
    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    debugImplementation(libs.androidx.compose.ui.tooling)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
}
