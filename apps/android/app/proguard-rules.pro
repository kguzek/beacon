# JNA and the UniFFI bindings are reached through reflection and from native
# code; keep them intact or calls into the Rust core fail at runtime.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uk.guzek.beacon.core.** { *; }
-dontwarn java.awt.**
