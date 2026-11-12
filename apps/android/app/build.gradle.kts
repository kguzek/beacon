import com.android.build.api.variant.FilterConfiguration

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

/** Released as `android-v<appVersion>`; CD checks that the tag matches. */
val appVersion = "0.1.0"

/** e.g. 1.2.3 → 10203. */
val baseVersionCode =
    appVersion.split('.').map(String::toInt).let { (major, minor, patch) ->
        major * 10_000 + minor * 100 + patch
    }

/** ABI → last digit of the version code, so each per-ABI APK upgrades cleanly. */
val abiCodes = mapOf("armeabi-v7a" to 1, "arm64-v8a" to 2, "x86_64" to 3)

/** `./build.sh bindings` output: jniLibs, Kotlin bindings and a host library for unit tests. */
val generated = rootProject.layout.projectDirectory.dir("generated")

android {
    namespace = "uk.guzek.beacon"
    compileSdk = 37
    ndkVersion = libs.versions.ndk.get()

    defaultConfig {
        applicationId = "uk.guzek.beacon"
        minSdk = 28
        targetSdk = 37
        versionName = appVersion
        versionCode = baseVersionCode
    }

    signingConfigs {
        // CD provides the upload keystore through the environment. Without it,
        // release APKs stay unsigned (as F-Droid builds them).
        val keystore = providers.environmentVariable("BEACON_KEYSTORE").orNull
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = providers.environmentVariable("BEACON_KEYSTORE_PASSWORD").get()
                keyAlias = providers.environmentVariable("BEACON_KEY_ALIAS").get()
                keyPassword = providers.environmentVariable("BEACON_KEY_PASSWORD").get()
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            signingConfig = signingConfigs.findByName("release")
            // Reproducible builds: no commit hash in the APK.
            vcsInfo.include = false
        }
    }

    splits {
        abi {
            isEnable = true
            reset()
            include("armeabi-v7a", "arm64-v8a", "x86_64")
            isUniversalApk = false
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(generated.dir("jniLibs").asFile.path)
            kotlin.directories.add(generated.dir("kotlin").asFile.path)
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    // F-Droid rejects the Google-encrypted dependency list.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    lint {
        abortOnError = true
        warningsAsErrors = true
        checkReleaseBuilds = false
        // Dependency updates come from Dependabot, not lint.
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion")
        // Sideloaded (F-Droid / Obtainium) app: these Play policies don't apply.
        disable += setOf("ExactAlarm", "BatteryLife")
    }

    testOptions { unitTests.isReturnDefaultValues = true }
}

androidComponents {
    onVariants { variant ->
        variant.outputs.forEach { output ->
            val abi =
                output.filters
                    .single { it.filterType == FilterConfiguration.FilterType.ABI }
                    .identifier
            output.versionCode.set(baseVersionCode * 10 + abiCodes.getValue(abi))
        }
    }
}

val checkBindings =
    tasks.register("checkBindings") {
        description = "Fails early when ./build.sh bindings hasn't been run."
        val bindings = generated.file("kotlin/uk/guzek/beacon/core/beacon_ffi.kt").asFile
        doLast {
            check(bindings.exists()) {
                "Missing Rust bindings ($bindings): run ./build.sh bindings first"
            }
        }
    }

tasks.named("preBuild") { dependsOn(checkBindings) }

tasks.withType<Test>().configureEach {
    // Lets unit tests call the real Rust core through the host library.
    systemProperty("jna.library.path", generated.dir("host").asFile.path)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(variantOf(libs.jna) { artifactType("aar") })

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    debugImplementation(libs.compose.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.jna)
}
