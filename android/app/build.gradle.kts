import org.gradle.process.ExecOperations
import javax.inject.Inject

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.osvauld.p2p"
    compileSdk = 35
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "com.osvauld.tinline"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("com.google.zxing:core:3.5.3")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
}

// ---- Rust (cargo-ndk + uniffi-bindgen) wiring ----
val repoRoot = rootProject.projectDir.parentFile
val sdkDir: String = System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
    ?: "${System.getProperty("user.home")}/Android/Sdk"
val ndkDir = "$sdkDir/ndk/28.2.13676358"
val rustAbis = (findProperty("abis") as String? ?: "x86_64,arm64-v8a").split(",").map { it.trim() }
val rustRelease = (findProperty("rustRelease") as String? ?: "true").toBoolean()
val profileDir = if (rustRelease) "release" else "debug"
val jniOut = layout.buildDirectory.dir("rustJniLibs")
val uniffiOut = layout.buildDirectory.dir("generated/uniffi/kotlin")
val cargoBin = "${System.getProperty("user.home")}/.cargo/bin"

abstract class RustExec @Inject constructor(@get:Internal val ops: ExecOperations) : DefaultTask() {
    @get:Input abstract val cmd: ListProperty<String>
    @get:Input abstract val workDir: Property<String>
    @get:Input abstract val extraEnv: MapProperty<String, String>
    @TaskAction fun run() {
        ops.exec {
            workingDir(workDir.get())
            environment(extraEnv.get())
            commandLine(cmd.get())
        }
    }
}

val rustInputs = files(
    fileTree(File(repoRoot, "crates")) { exclude("**/target/**") },
    File(repoRoot, "Cargo.toml"), File(repoRoot, "Cargo.lock"),
)
val envMap = mapOf(
    "ANDROID_NDK_HOME" to ndkDir,
    "PATH" to "$cargoBin:${System.getenv("PATH")}",
)

val cargoNdkBuild = tasks.register<RustExec>("cargoNdkBuild") {
    workDir.set(repoRoot.absolutePath)
    extraEnv.set(envMap)
    cmd.set(buildList {
        add("cargo"); add("ndk")
        rustAbis.forEach { add("-t"); add(it) }
        addAll(listOf("-P", "28", "-o", jniOut.get().asFile.absolutePath, "build", "-p", "p2pcore"))
        if (rustRelease) add("--release")
    })
    inputs.files(rustInputs).withPropertyName("rustSources").withPathSensitivity(PathSensitivity.RELATIVE)
    outputs.dir(jniOut)
}

val uniffiBindgen = tasks.register<RustExec>("uniffiBindgen") {
    dependsOn(cargoNdkBuild)
    workDir.set(repoRoot.absolutePath)
    extraEnv.set(envMap)
    val so = File(repoRoot, "target/x86_64-linux-android/$profileDir/libp2pcore.so")
    cmd.set(listOf("cargo", "run", "-q", "-p", "p2pcore", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", so.absolutePath, "--language", "kotlin",
        "--out-dir", uniffiOut.get().asFile.absolutePath))
    inputs.files(rustInputs).withPropertyName("rustSources").withPathSensitivity(PathSensitivity.RELATIVE)
    outputs.dir(uniffiOut)
    doFirst { uniffiOut.get().asFile.deleteRecursively() }
}

// x86_64 must always be built (bindgen reads it)
if ("x86_64" !in rustAbis) throw GradleException("-Pabis must include x86_64")

android.sourceSets["main"].kotlin.srcDir(uniffiOut)
tasks.configureEach {
    if (name == "preBuild") dependsOn(uniffiBindgen)
}
tasks.matching { it.name.startsWith("compile") && it.name.endsWith("Kotlin") }.configureEach { dependsOn(uniffiBindgen) }
tasks.matching { it.name.contains("JniLibFolders") || it.name.startsWith("merge") && it.name.endsWith("NativeLibs") }.configureEach { dependsOn(cargoNdkBuild) }
