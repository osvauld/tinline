plugins {
    // AGP 9 compiles Kotlin itself (built-in Kotlin); only the Compose compiler plugin is separate.
    id("com.android.application") version "9.4.1" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.21" apply false
}
