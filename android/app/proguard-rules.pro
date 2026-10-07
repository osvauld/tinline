# JNA loads its native dispatch and reflects over Structure/Callback fields.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-dontwarn java.awt.**
-dontwarn com.sun.jna.**
# UniFFI-generated bindings are reached through JNA by name.
-keep class uniffi.** { *; }
# Core event callbacks implemented by the app, called from Rust via JNA.
-keep class com.osvauld.p2p.P2pApp { *; }
-keep class * implements uniffi.p2pcore.NodeEvents { *; }
# zxing-android-embedded uses reflection-free code; nothing else to keep.
