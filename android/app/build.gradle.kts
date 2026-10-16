plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.veloren.client"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.veloren.client"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters.add("arm64-v8a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    kotlinOptions {
        jvmTarget = "1.8"
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

tasks.register("copyVelorenAssets") {
    val projectRoot = rootProject.projectDir
    val srcDir = projectRoot.resolve("assets")
    val dstDir = file("src/main/assets")

    doFirst {
        dstDir.deleteRecursively()
        dstDir.mkdirs()
    }

    doLast {
        if (srcDir.exists()) {
            srcDir.walkTopDown().forEach { srcFile ->
                val relativePath = srcDir.relativeTo(srcFile).toString()
                if (relativePath.isNotEmpty()) {
                    val dstFile = dstDir.resolve(relativePath)
                    if (srcFile.isDirectory) {
                        dstFile.mkdirs()
                    } else {
                        dstFile.parentFile.mkdirs()
                        srcFile.copyTo(dstFile, overwrite = true)
                    }
                }
            }
        }
    }
}

dependencies {
    implementation("org.jetbrains.kotlin:kotlin-stdlib:2.0.21")
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
}
