#!/bin/bash

set -ex -o pipefail

cd "$(dirname "$0")"

if [[ -z $ANDROID_TOOLCHAINS ]]; then
    echo 'missing ANDROID_TOOLCHAINS env var'
    exit 1
fi

export AR="$ANDROID_TOOLCHAINS/llvm-ar"

export CC="$ANDROID_TOOLCHAINS/aarch64-linux-android30-clang"
cargo build --target aarch64-linux-android --release

export CC="$ANDROID_TOOLCHAINS/armv7a-linux-androideabi30-clang"
cargo build --target armv7-linux-androideabi --release

export CC="$ANDROID_TOOLCHAINS/i686-linux-android30-clang"
cargo build --target i686-linux-android --release

export CC="$ANDROID_TOOLCHAINS/x86_64-linux-android30-clang"
cargo build --target x86_64-linux-android --release

unset CC
unset AR

LIB_PATH="../app/src/main/jniLibs"
mkdir -p "$LIB_PATH"
mkdir -p "$LIB_PATH/arm64-v8a"
mkdir -p "$LIB_PATH/armeabi-v7a"
mkdir -p "$LIB_PATH/x86"
mkdir -p "$LIB_PATH/x86_64"

cp target/aarch64-linux-android/release/librammingen_android.so \
    "$LIB_PATH/arm64-v8a/"
cp target/armv7-linux-androideabi/release/librammingen_android.so \
    "$LIB_PATH/armeabi-v7a/"
cp target/i686-linux-android/release/librammingen_android.so \
    "$LIB_PATH/x86/"
cp target/x86_64-linux-android/release/librammingen_android.so \
    "$LIB_PATH/x86_64/"

#cd ..
#rm -r app/build
#gradle build
