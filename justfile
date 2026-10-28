default: lint test

build:
    cargo build --workspace --all-targets

test:
    cargo test --workspace

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo fmt --all --check

fmt:
    cargo fmt --all

fuzz target="packet_view":
    cargo +nightly fuzz run {{target}}

# Builds the Android .so straight into the Gradle module's jniLibs directory. The `android`
# profile is release with `panic = "abort"`.
android:
    cargo ndk -t arm64-v8a --platform 29 -o ../crownconnect-android/app/src/main/jniLibs build --profile android -p llts-android

# Fails unless the .so exports exactly the `external fun`s Llts.kt declares.
android-symbols: android
    #!/usr/bin/env bash
    set -euo pipefail
    ndk="${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}"
    nm="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-nm"
    library=../crownconnect-android/app/src/main/jniLibs/arm64-v8a/libllts_android.so
    kotlin=../crownconnect-android/app/src/main/java/com/crownos/connect/llts/Llts.kt
    diff \
        <("$nm" -D --defined-only "$library" | awk '{ print $3 }' | grep '^Java_' | sed 's/^Java_com_crownos_connect_llts_Llts_//' | sort) \
        <(grep -o 'external fun native[A-Za-z]*' "$kotlin" | sed 's/^external fun //' | sort)
    echo "JNI symbols match Llts.kt"

# The CI gate: fails when p99 protocol latency leaves its budget or a frame is lost.
bench *ARGS:
    cargo run --release -q -p llts-bench -- {{ARGS}}

# Sweeps the loss rates the repair policy is meant to cover.
bench-sweep:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -q -p llts-bench
    for drop in 0 100 50 20; do
        ./target/release/llts-bench --frames 300 --drop-every "$drop"
        echo
    done
