cargo ndk --target arm64-v8a build --release $1
mkdir -p ../ywallet/android/app/src/main/jniLibs/arm64-v8a
cp ../target/aarch64-linux-android/release/libzcash_warp.so ../ywallet/android/app/src/main/jniLibs/arm64-v8a/
cargo ndk --target armeabi-v7a build --release $1
mkdir -p ../ywallet/android/app/src/main/jniLibs/armeabi-v7a
cp ../target/armv7-linux-androideabi/release/libzcash_warp.so ../ywallet/android/app/src/main/jniLibs/armeabi-v7a/
