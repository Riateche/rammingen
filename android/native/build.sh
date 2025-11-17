#!/bin/bash

set -e -o pipefail

cd "$(dirname "$0")"

if [[ -z $ANDROID_TOOLCHAINS ]]; then
    echo 'missing ANDROID_TOOLCHAINS env var'
    exit 1
fi

export AR="$ANDROID_TOOLCHAINS/llvm-ar"

export CC="$ANDROID_TOOLCHAINS/aarch64-linux-android30-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC"
cargo build --target aarch64-linux-android --release

export CC="$ANDROID_TOOLCHAINS/armv7a-linux-androideabi30-clang"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$CC"
cargo build --target armv7-linux-androideabi --release

export CC="$ANDROID_TOOLCHAINS/i686-linux-android30-clang"
export CARGO_TARGET_I686_LINUX_ANDROID_LINKER="$CC"
cargo build --target i686-linux-android --release

export CC="$ANDROID_TOOLCHAINS/x86_64-linux-android30-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$CC"
cargo build --target x86_64-linux-android --release

unset CC
unset AR

LIB_PATH="../app/src/main/jniLibs"
mkdir -p "$LIB_PATH"
mkdir -p "$LIB_PATH/arm64-v8a"
mkdir -p "$LIB_PATH/armeabi-v7a"
mkdir -p "$LIB_PATH/x86"
mkdir -p "$LIB_PATH/x86_64"

cp ../../target/aarch64-linux-android/release/librammingen_android.so \
    "$LIB_PATH/arm64-v8a/"
cp ../../target/armv7-linux-androideabi/release/librammingen_android.so \
    "$LIB_PATH/armeabi-v7a/"
cp ../../target/i686-linux-android/release/librammingen_android.so \
    "$LIB_PATH/x86/"
cp ../../target/x86_64-linux-android/release/librammingen_android.so \
    "$LIB_PATH/x86_64/"
