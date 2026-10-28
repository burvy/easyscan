# flutter_rust_bridge talks to the Rust library through dart:ffi, so there is no
# Java/Kotlin to keep for it. Flutter and the AdMob/UMP SDKs ship their own
# consumer rules. Add -keep rules here only if a release run shows a failure.

# WorkManager (pulled in by the AdMob SDK) builds its Room database by
# reflection on the generated *_Impl class's no-arg constructor; R8 strips that
# constructor, and the app then crashes at launch inside InitializationProvider.
-keepclassmembers class * extends androidx.room.RoomDatabase {
    <init>();
}
