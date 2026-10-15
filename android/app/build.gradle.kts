import org.gradle.process.ExecOperations
import javax.inject.Inject

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Controlled quality experiment. Opt-in debug only; no release/default preference change.
val videoHdCapture = (findProperty("videoHdCapture") as String? ?: "false").toBooleanStrictOrNull()
    ?: throw GradleException("-PvideoHdCapture must be true or false")

android {
    namespace = "com.osvauld.p2p"
    // Compose 1.12 needs API 37 to compile against; targetSdk (runtime behaviour) stays 36.
    compileSdk { version = release(37) { minorApiLevel = 2 } }
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "com.osvauld.tinline"
        minSdk = 28
        targetSdk = 36
        buildConfigField("boolean", "VIDEO_HD_CAPTURE", "false")
        // Play needs a higher code on every upload; scripts/build_android.py --bundle passes the commit count.
        versionCode = (findProperty("versionCode") as String?)?.toInt() ?: 580
        versionName = (findProperty("versionName") as String?) ?: "0.1.32"
        // Only ship ABIs that have libp2pcore: libraries like JNA bring 32-bit/mips copies, which
        // would make Play offer the app to devices where the core can't load.
        ndk { abiFilters += (findProperty("abis") as String? ?: "x86_64,arm64-v8a").split(",").map { it.trim() } }
    }
    // Release signing: set RELEASE_STORE_FILE / RELEASE_STORE_PASSWORD / RELEASE_KEY_ALIAS /
    // RELEASE_KEY_PASSWORD in ~/.gradle/gradle.properties (or -P). Without them the release build
    // is signed with the DEBUG key so it can be installed locally for testing -- NOT publishable.
    val relStore = findProperty("RELEASE_STORE_FILE") as String?
    if ((findProperty("requireReleaseSigning") as String?) == "true") {
        listOf("RELEASE_STORE_FILE", "RELEASE_STORE_PASSWORD", "RELEASE_KEY_ALIAS", "RELEASE_KEY_PASSWORD")
            .forEach { if ((findProperty(it) as String?).isNullOrBlank()) throw GradleException("Missing release signing property: $it") }
    }
    signingConfigs {
        if (relStore != null) create("release") {
            storeFile = file(relStore)
            storePassword = findProperty("RELEASE_STORE_PASSWORD") as String?
            keyAlias = findProperty("RELEASE_KEY_ALIAS") as String?
            keyPassword = findProperty("RELEASE_KEY_PASSWORD") as String?
        }
    }
    buildTypes {
        getByName("debug") {
            buildConfigField("boolean", "VIDEO_HD_CAPTURE", videoHdCapture.toString())
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = if (relStore != null) signingConfigs.getByName("release") else signingConfigs.getByName("debug")
        }
    }
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    // A plain path, not a Provider: AGP 9 rejects providers here (android.sourceset.disallowProvider).
    sourceSets["main"].jniLibs.directories.add(layout.buildDirectory.dir("rustJniLibs").get().asFile.path)
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.core:core-ktx:1.19.1")
    // Registers calls with Android Telecom: Bluetooth/wired routing, headset buttons, cars, watches, hold.
    implementation("androidx.core:core-telecom:1.0.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("com.google.zxing:core:3.5.4")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    // JVM unit tests (src/test): the placeholder groups & privacy state machine.
    testImplementation("junit:junit:4.13.2")
    // Call video: ImageAnalysis (YUV_420_888 to the core) + Preview (self-view).
    implementation("androidx.camera:camera-camera2:1.6.2")
    implementation("androidx.camera:camera-lifecycle:1.6.2")
    implementation("androidx.camera:camera-view:1.6.2")
}

// ---- Rust (cargo-ndk + uniffi-bindgen) wiring ----
val repoRoot = rootProject.projectDir.parentFile
val sdkDir: String = System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
    ?: "${System.getProperty("user.home")}/Android/Sdk"
val ndkDir = "$sdkDir/ndk/28.2.13676358"
val rustAbis = (findProperty("abis") as String? ?: "x86_64,arm64-v8a").split(",").map { it.trim() }
require(rustAbis.isNotEmpty() && rustAbis.all { it in setOf("x86_64", "arm64-v8a") }) {
    "-Pabis must contain only x86_64 and/or arm64-v8a"
}
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
// Static libaom per ABI (video encoder), built by scripts/build_libaom.py; the video crate's
// build.rs reads AOM_DIR_<target triple>.
val aomRoot = findProperty("aomRoot") as String? ?: "${System.getProperty("user.home")}/build-cache/p2p_chat/libaom"
// One pin: the version lives in scripts/build_libaom.py.
val aomTag = "v" + Regex("""^VERSION = "([^"]+)"""", RegexOption.MULTILINE)
    .find(File(repoRoot, "scripts/build_libaom.py").readText())!!.groupValues[1]
val abiTriple = mapOf("arm64-v8a" to "aarch64-linux-android", "x86_64" to "x86_64-linux-android")
val envMap = mapOf(
    "ANDROID_NDK_HOME" to ndkDir,
    "PATH" to "$cargoBin:${System.getenv("PATH")}",
) + rustAbis.mapNotNull { abiTriple[it] }.associate {
    "AOM_DIR_${it.replace('-', '_')}" to "$aomRoot/install-$aomTag-$it"
}

val cargoNdkBuild = tasks.register<RustExec>("cargoNdkBuild") {
    workDir.set(repoRoot.absolutePath)
    extraEnv.set(envMap)
    cmd.set(buildList {
        add("cargo"); add("ndk")
        rustAbis.forEach { add("-t"); add(it) }
        // --lib: only libp2pcore.so is linked; without it cargo also builds the crate's bins (search-bench, year-bench), which is slow.
        addAll(listOf("-P", "28", "-o", jniOut.get().asFile.absolutePath, "build", "--locked", "-p", "p2pcore", "--lib"))
        if (rustRelease) add("--release")
    })
    inputs.files(rustInputs).withPropertyName("rustSources").withPathSensitivity(PathSensitivity.RELATIVE)
    outputs.dir(jniOut)
    // Only libp2pcore is linked. cargo-ndk also copies stale .so files it finds in target/ (old
    // iroh dylibs), which would otherwise be packaged too.
    doFirst { jniOut.get().asFile.deleteRecursively() }
    doLast { jniOut.get().asFile.walk().filter { it.isFile && it.name != "libp2pcore.so" }.forEach { it.delete() } }
}

val uniffiBindgen = tasks.register<RustExec>("uniffiBindgen") {
    dependsOn(cargoNdkBuild)
    workDir.set(repoRoot.absolutePath)
    extraEnv.set(envMap)
    // Bindgen reads metadata from the object file; it does not load/run the target library.
    // Use an ABI we actually built, so arm64-only packages need no x86_64 build.
    val so = File(repoRoot, "target/${abiTriple.getValue(rustAbis.first())}/$profileDir/libp2pcore.so")
    cmd.set(listOf("cargo", "run", "--locked", "-q", "-p", "p2pcore", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", so.absolutePath, "--language", "kotlin",
        "--out-dir", uniffiOut.get().asFile.absolutePath))
    inputs.files(rustInputs).withPropertyName("rustSources").withPathSensitivity(PathSensitivity.RELATIVE)
    outputs.dir(uniffiOut)
    doFirst { uniffiOut.get().asFile.deleteRecursively() }
}

android.sourceSets["main"].kotlin.directories.add(uniffiOut.get().asFile.path)
// The Tinline line icons, generated by crates/tin-icons (TinIcons.kt); shared with the desktop.
android.sourceSets["main"].kotlin.directories.add(rootDir.resolve("../crates/tin-icons/generated").path)
tasks.configureEach {
    if (name == "preBuild") dependsOn(uniffiBindgen)
}
tasks.matching { it.name.startsWith("compile") && it.name.endsWith("Kotlin") }.configureEach { dependsOn(uniffiBindgen) }
tasks.matching { it.name.contains("JniLibFolders") || it.name.startsWith("merge") && it.name.endsWith("NativeLibs") }.configureEach { dependsOn(cargoNdkBuild) }
