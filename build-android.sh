#!/usr/bin/env bash
# ==============================================================================
# Obscura Android - Automated Release APK Build Script
# Targets: Linux Servers, Google Colab, Cloud VMs (Ubuntu 20.04 / 22.04 / 24.04)
# Repo: https://github.com/computerengineering-tutorial/obscura-android
# ==============================================================================

set -euo pipefail

echo "=========================================================="
echo "🚀 Obscura Android Build Script Starting..."
echo "=========================================================="

# ------------------------------------------------------------------------------
# 1. Workspace / Repository Setup
# ------------------------------------------------------------------------------
REPO_URL="https://github.com/computerengineering-tutorial/obscura-android.git"
REPO_DIR="obscura-android"

if [ ! -f "Cargo.toml" ] || ! grep -q "obscura-android" "Cargo.toml" 2>/dev/null; then
    if [ ! -d "$REPO_DIR" ]; then
        echo "📥 Cloning repository from ${REPO_URL}..."
        git clone "$REPO_URL" "$REPO_DIR"
    fi
    cd "$REPO_DIR"
fi

WORKSPACE_ROOT="$(pwd)"
echo "📂 Working directory: ${WORKSPACE_ROOT}"

# ------------------------------------------------------------------------------
# 2. System Packages & Clang/LLVM
# ------------------------------------------------------------------------------
echo "📦 Installing system build dependencies..."
if command -v apt-get >/dev/null 2>&1; then
    # Disable docker-clean to allow caching on Debian/Ubuntu systems
    sudo rm -f /etc/apt/apt.conf.d/docker-clean 2>/dev/null || true
    sudo apt-get update -qq

    # Install core tools, JDK 17, and LLVM/Clang
    sudo apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        ninja-build \
        pkg-config \
        libglib2.0-dev \
        unzip \
        curl \
        git \
        python3 \
        openjdk-17-jdk \
        clang \
        libclang-dev \
        lld \
        llvm-dev \
        ca-certificates
fi

# Install sccache for fast C++/Rust compilation caching if not present
if ! command -v sccache >/dev/null 2>&1; then
    echo "⚡ Installing sccache for ultra-fast compilation caching..."
    SCCACHE_VER="v0.8.2"
    curl -fsSL "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VER}/sccache-${SCCACHE_VER}-x86_64-unknown-linux-musl.tar.gz" | tar -xz -C /tmp 2>/dev/null || true
    if [ -f "/tmp/sccache-${SCCACHE_VER}-x86_64-unknown-linux-musl/sccache" ]; then
        sudo cp "/tmp/sccache-${SCCACHE_VER}-x86_64-unknown-linux-musl/sccache" /usr/local/bin/sccache || cp "/tmp/sccache-${SCCACHE_VER}-x86_64-unknown-linux-musl/sccache" "$HOME/.cargo/bin/sccache"
        chmod +x /usr/local/bin/sccache 2>/dev/null || chmod +x "$HOME/.cargo/bin/sccache" 2>/dev/null || true
    fi
    rm -rf /tmp/sccache* 2>/dev/null || true
fi

# Detect Java 17
if [ -d "/usr/lib/jvm/java-17-openjdk-amd64" ]; then
    export JAVA_HOME="/usr/lib/jvm/java-17-openjdk-amd64"
elif [ -z "${JAVA_HOME:-}" ]; then
    JAVA_BIN="$(which java 2>/dev/null || true)"
    if [ -n "$JAVA_BIN" ]; then
        export JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$JAVA_BIN")")")"
    fi
fi
if [ -n "${JAVA_HOME:-}" ]; then
    export PATH="$JAVA_HOME/bin:$PATH"
fi
echo "☕ Java: $(java -version 2>&1 | head -n 1)"

# Detect LIBCLANG_PATH for bindgen
if [ -z "${LIBCLANG_PATH:-}" ]; then
    LIBCLANG_SO=$(find /usr/lib/llvm* /usr/lib -name "libclang.so*" 2>/dev/null | head -n 1 || true)
    if [ -n "$LIBCLANG_SO" ]; then
        export LIBCLANG_PATH="$(dirname "$LIBCLANG_SO")"
    fi
fi
echo "🔧 LIBCLANG_PATH: ${LIBCLANG_PATH:-not set (defaulting to system)}"

# ------------------------------------------------------------------------------
# 3. Rust Toolchain Setup
# ------------------------------------------------------------------------------
echo "🦀 Setting up Rust toolchain..."
if ! command -v rustc >/dev/null 2>&1; then
    echo "Installing rustup and stable toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source "$HOME/.cargo/env"
fi

if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

rustup default stable
rustup target add aarch64-linux-android

# Install cargo-ndk
if ! command -v cargo-ndk >/dev/null 2>&1; then
    echo "Installing cargo-ndk..."
    cargo install cargo-ndk --locked
fi

# ------------------------------------------------------------------------------
# 4. Android SDK & NDK Setup
# ------------------------------------------------------------------------------
echo "📱 Setting up Android SDK & NDK r26d..."

# Android SDK
if [ -z "${ANDROID_HOME:-}" ] || [ ! -d "${ANDROID_HOME}" ]; then
    export ANDROID_HOME="$HOME/android-sdk"
    export ANDROID_SDK_ROOT="$ANDROID_HOME"
    mkdir -p "$ANDROID_HOME/cmdline-tools"

    if [ ! -d "$ANDROID_HOME/cmdline-tools/latest" ]; then
        echo "⬇️ Downloading Android commandlinetools..."
        CMDLINE_ZIP="/tmp/cmdline-tools.zip"
        curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip" -o "$CMDLINE_ZIP"
        unzip -q "$CMDLINE_ZIP" -d "/tmp/cmdline-tools"
        mkdir -p "$ANDROID_HOME/cmdline-tools/latest"
        cp -r /tmp/cmdline-tools/cmdline-tools/* "$ANDROID_HOME/cmdline-tools/latest/"
        rm -rf "$CMDLINE_ZIP" /tmp/cmdline-tools
    fi

    export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"

    # Accept licenses and install platform 34 + build-tools
    echo "Accepting Android SDK licenses and installing platform-34..."
    yes | "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --licenses >/dev/null 2>&1 || true
    "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" "platforms;android-34" "build-tools;34.0.0" "platform-tools" >/dev/null 2>&1 || true
else
    export ANDROID_SDK_ROOT="$ANDROID_HOME"
    export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
fi

# Accept licenses directly if directory exists
mkdir -p "$ANDROID_HOME/licenses"
echo "24333f8a63b6825ea9c5514f83c2829b004d1fee" > "$ANDROID_HOME/licenses/android-sdk-license" 2>/dev/null || true
echo "84831b9409646a918e30573bab4c9c91346d8abd" >> "$ANDROID_HOME/licenses/android-sdk-license" 2>/dev/null || true
echo "d56f5187479451eabf01fb78af6dfcb131a6481e" >> "$ANDROID_HOME/licenses/android-sdk-license" 2>/dev/null || true

# Android NDK r26d
NDK_TARGET_DIR="$HOME/android-ndk-r26d"
if [ -n "${ANDROID_NDK_HOME:-}" ] && [ -d "${ANDROID_NDK_HOME}" ]; then
    NDK_TARGET_DIR="$ANDROID_NDK_HOME"
fi

if [ ! -d "$NDK_TARGET_DIR" ]; then
    echo "⬇️ Downloading Android NDK r26d (~630MB)..."
    NDK_ZIP="/tmp/android-ndk-r26d.zip"
    curl -fsSL "https://dl.google.com/android/repository/android-ndk-r26d-linux.zip" -o "$NDK_ZIP"
    echo "Unzipping NDK..."
    unzip -q "$NDK_ZIP" -d "$HOME"
    rm -f "$NDK_ZIP"
fi

export ANDROID_NDK_HOME="$NDK_TARGET_DIR"
export NDK_HOME="$NDK_TARGET_DIR"
export ANDROID_NDK_ROOT="$NDK_TARGET_DIR"
export PATH="$ANDROID_NDK_HOME:$PATH"
echo "✅ Android NDK configured at: $ANDROID_NDK_HOME"

# ------------------------------------------------------------------------------
# 5. Build Concurrency & Compiler Environment (Maximum Performance Mode)
# ------------------------------------------------------------------------------
# Check and create swap memory if running on a memory-constrained VM (e.g. Google Colab)
# This prevents V8 C++ clang processes from hitting the OOM killer when using high concurrency
TOTAL_SWAP=$(free -m 2>/dev/null | awk '/^Swap:/{print $2}' || echo "0")
if [ "${TOTAL_SWAP:-0}" -lt 4096 ]; then
    if command -v sudo >/dev/null 2>&1; then
        echo "💾 Low swap detected. Creating 16GB swapfile to enable maximum parallel compilation..."
        sudo fallocate -l 16G /swapfile 2>/dev/null || sudo dd if=/dev/zero of=/swapfile bs=1M count=16384 2>/dev/null || true
        if [ -f /swapfile ]; then
            sudo chmod 600 /swapfile 2>/dev/null || true
            sudo mkswap /swapfile >/dev/null 2>&1 || true
            sudo swapon /swapfile >/dev/null 2>&1 || true
            echo "✅ 16GB swap active."
        fi
    fi
fi

RAM_GB=$(free -g 2>/dev/null | awk '/^Mem:/{print $2}' || echo "8")
CPUS=$(nproc 2>/dev/null || echo "2")

# Allow manual override via JOBS=X, otherwise use all available CPU cores
BUILD_JOBS="${JOBS:-$CPUS}"
if [ "$BUILD_JOBS" -lt 1 ]; then BUILD_JOBS=1; fi

export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS="${BUILD_JOBS}"
export NUM_JOBS="${BUILD_JOBS}"
export NINJAFLAGS="-j ${BUILD_JOBS}"
export OBSCURA_NO_SNAPSHOT="1"

# Set parallel codegen units to match build jobs (avoiding single-core bottleneck of codegen-units=1)
CODEGEN_UNITS="${BUILD_JOBS}"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C opt-level=3 -C target-feature=+neon -C codegen-units=${CODEGEN_UNITS} -C debuginfo=0 -C strip=symbols -C link-arg=-Wl,-O3 -C link-arg=-Wl,--gc-sections"

# Setup GN_ARGS for V8 (use_thin_lto=false drastically speeds up V8 link time)
GN_BASE='android_ndk_version="r26d" symbol_level=0 v8_symbol_level=0 blink_symbol_level=0 is_debug=false treat_warnings_as_errors=false enable_rust=false enable_rust_cxx=false v8_enable_temporal_support=false v8_enable_fast_mksnapshot=true use_thin_lto=false'

# Detect Google Drive or custom persistent cache directory
PERSISTENT_CACHE_DIR="${OBSCURA_CACHE_DIR:-}"
if [ -z "$PERSISTENT_CACHE_DIR" ] && [ -d "/content/drive/MyDrive" ]; then
    PERSISTENT_CACHE_DIR="/content/drive/MyDrive/obscura_build_cache"
fi

if [ -n "$PERSISTENT_CACHE_DIR" ]; then
    mkdir -p "$PERSISTENT_CACHE_DIR"
    echo "📁 Persistent cache enabled at: $PERSISTENT_CACHE_DIR"
fi

if command -v sccache >/dev/null 2>&1; then
    export RUSTC_WRAPPER="sccache"
    if [ -n "$PERSISTENT_CACHE_DIR" ]; then
        export SCCACHE_DIR="$PERSISTENT_CACHE_DIR/sccache"
    else
        export SCCACHE_DIR="$HOME/.cache/sccache"
    fi
    mkdir -p "$SCCACHE_DIR"
    export GN_ARGS="${GN_BASE} cc_wrapper=\"sccache\""
    export EXTRA_GN_ARGS="${GN_ARGS}"
    echo "⚡ sccache enabled at: $SCCACHE_DIR"
else
    export GN_ARGS="${GN_BASE}"
    export EXTRA_GN_ARGS="${GN_ARGS}"
fi

echo "⚙️ Build settings: CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}, NUM_JOBS=${NUM_JOBS}, CPUS=${CPUS}, RAM=${RAM_GB}GB"

# ------------------------------------------------------------------------------
# 6. ICU Data & Prebuilt V8 Cache
# ------------------------------------------------------------------------------
echo "🌐 Preparing ICU data..."
mkdir -p "$HOME/.cache/obscura-v8-icu"
if [ ! -s "$HOME/.cache/obscura-v8-icu/icudtl.dat" ]; then
    echo "Downloading V8 common icudtl.dat..."
    curl -fsSL "https://chromium.googlesource.com/chromium/deps/icu/+/ee5f27adc28bd3f15b2c293f726d14d2e336cbd5/common/icudtl.dat?format=TEXT" | base64 -d > "$HOME/.cache/obscura-v8-icu/icudtl.dat"
fi
if [ ! -s "$HOME/.cache/obscura-v8-icu/icudtl_android.dat" ]; then
    echo "Downloading V8 android icudtl.dat..."
    curl -fsSL "https://chromium.googlesource.com/chromium/deps/icu/+/ee5f27adc28bd3f15b2c293f726d14d2e336cbd5/android/icudtl.dat?format=TEXT" | base64 -d > "$HOME/.cache/obscura-v8-icu/icudtl_android.dat" || cp "$HOME/.cache/obscura-v8-icu/icudtl.dat" "$HOME/.cache/obscura-v8-icu/icudtl_android.dat"
fi

PREBUILT_DIR="$HOME/.cache/obscura-v8-prebuilt"
mkdir -p "$PREBUILT_DIR"
ARCHIVE_FILE="$PREBUILT_DIR/librusty_v8_release_aarch64-linux-android.a.gz"
BINDING_FILE="$PREBUILT_DIR/src_binding_release_aarch64-linux-android.rs"

# Check persistent cache (Google Drive / custom dir) first
if [ -n "$PERSISTENT_CACHE_DIR" ]; then
    if [ -s "$PERSISTENT_CACHE_DIR/librusty_v8_release_aarch64-linux-android.a.gz" ] && [ ! -s "$ARCHIVE_FILE" ]; then
        echo "⚡ Restoring prebuilt V8 from persistent cache ($PERSISTENT_CACHE_DIR)..."
        cp "$PERSISTENT_CACHE_DIR/librusty_v8_release_aarch64-linux-android.a.gz" "$ARCHIVE_FILE"
    fi
    if [ -s "$PERSISTENT_CACHE_DIR/src_binding_release_aarch64-linux-android.rs" ] && [ ! -s "$BINDING_FILE" ]; then
        cp "$PERSISTENT_CACHE_DIR/src_binding_release_aarch64-linux-android.rs" "$BINDING_FILE"
    fi
fi

if [ ! -s "$ARCHIVE_FILE" ] || [ ! -s "$BINDING_FILE" ]; then
    echo "Checking for prebuilt V8 binary release..."
    V8_RELEASE_URL="https://github.com/computerengineering-tutorial/obscura-android/releases/download/v8-android-v150.4.0"
    if curl -fsSL -I "$V8_RELEASE_URL/librusty_v8_release_aarch64-linux-android.a.gz" >/dev/null 2>&1; then
        echo "⚡ Downloading prebuilt V8 from GitHub release..."
        curl -fsSL "$V8_RELEASE_URL/librusty_v8_release_aarch64-linux-android.a.gz" -o "$ARCHIVE_FILE"
        curl -fsSL "$V8_RELEASE_URL/src_binding_release_aarch64-linux-android.rs" -o "$BINDING_FILE"
    fi
fi

# ------------------------------------------------------------------------------
# 7. Fetch Dependencies & Patch Crates for Android Cross-Compilation
# ------------------------------------------------------------------------------
echo "📥 Fetching cargo dependencies for aarch64-linux-android..."
cargo fetch --target aarch64-linux-android

# Unpack v8 and deno_core crates from cache if needed
for crate in $(find "$HOME/.cargo/registry/cache/" -name "v8-150*.crate" 2>/dev/null || true); do
    src_dir="${crate%/cache/*}/src/${crate##*/cache/}"
    src_dir="${src_dir%/*}/v8-150.4.0"
    mkdir -p "$src_dir"
    tar -xzf "$crate" -C "$src_dir" --strip-components=1 2>/dev/null || true
done
for crate in $(find "$HOME/.cargo/registry/cache/" -name "deno_core-0.412*.crate" 2>/dev/null || true); do
    src_dir="${crate%/cache/*}/src/${crate##*/cache/}"
    src_dir="${src_dir%/*}/deno_core-0.412.0"
    mkdir -p "$src_dir"
    tar -xzf "$crate" -C "$src_dir" --strip-components=1 2>/dev/null || true
done

# Patch v8 and deno_core build files
echo "🩹 Applying cross-compilation patches to crates..."
for v8_dir in $(find "$HOME/.cargo/registry/src/" -type d -name "v8-*" 2>/dev/null || true); do
    p="$v8_dir/build.rs"
    if [ -f "$p" ]; then
        sed -z -i 's@fn static_lib_url() -> String {\n  if let Ok(custom_archive) = env::var("RUSTY_V8_ARCHIVE") {\n    return custom_archive;\n  }@fn static_lib_url() -> String {\n  let target = env::var("TARGET").unwrap_or_default();\n  let host = env::var("HOST").unwrap_or_default();\n  if target != host || target.contains("android") {\n    if let Ok(custom_archive) = env::var("RUSTY_V8_ARCHIVE") {\n      return custom_archive;\n    }\n  }@' "$p" || true
        sed -z -i 's@fn print_prebuilt_src_binding_path() {\n  if let Ok(binding) = env::var("RUSTY_V8_SRC_BINDING_PATH") {\n    println!("cargo:rustc-env=RUSTY_V8_SRC_BINDING_PATH={binding}");\n    return;\n  }@fn print_prebuilt_src_binding_path() {\n  let target = env::var("TARGET").unwrap_or_default();\n  let host = env::var("HOST").unwrap_or_default();\n  if target != host || target.contains("android") {\n    if let Ok(binding) = env::var("RUSTY_V8_SRC_BINDING_PATH") {\n      println!("cargo:rustc-env=RUSTY_V8_SRC_BINDING_PATH={binding}");\n      return;\n    }\n  }@' "$p" || true
    fi
done

for deno_dir in $(find "$HOME/.cargo/registry/src/" -type d -name "deno_core-*" 2>/dev/null || true); do
    p="$deno_dir/uv_compat/tty.rs"
    if [ -f "$p" ]; then
        sed -i '/#\[cfg(not(any(target_os = "macos", target_os = "linux")))\]/c\  #[cfg(target_os = "android")]\n  fn errno_location() -> *mut c_int {\n    unsafe extern "C" {\n      fn __errno() -> *mut c_int;\n    }\n    unsafe { __errno() }\n  }\n\n  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]' "$p" || true
    fi
done

# If prebuilt archive is not present, configure V8 C++ source tree
if [ -s "$ARCHIVE_FILE" ] && [ -s "$BINDING_FILE" ]; then
    echo "⚡ Prebuilt V8 archive found! Fast build enabled."
else
    echo "⚙️ Preparing full V8 source configuration..."
    for v8_dir in $(find "$HOME/.cargo/registry/src/" -type d -name "v8-*" 2>/dev/null || true); do
        mkdir -p "$v8_dir/third_party/icu/common" "$v8_dir/third_party/icu/android"
        cp "$HOME/.cache/obscura-v8-icu/icudtl.dat" "$v8_dir/third_party/icu/common/icudtl.dat"
        cp "$HOME/.cache/obscura-v8-icu/icudtl_android.dat" "$v8_dir/third_party/icu/android/icudtl.dat"

        mkdir -p "$v8_dir/third_party/android_toolchain"
        ln -sfn "$ANDROID_NDK_HOME" "$v8_dir/third_party/android_toolchain/ndk"
        ln -sfn "$ANDROID_NDK_HOME" "$v8_dir/third_party/android_ndk"

        mkdir -p "$v8_dir/third_party/android_platform"
        mkdir -p "$v8_dir/third_party/catapult"

        if [ -f "$v8_dir/build/config/android/BUILD.gn" ]; then
            if ! grep -q "android_ndk_version =" "$v8_dir/build/config/android/BUILD.gn"; then
                sed -i '1s/^/if (!defined(android_ndk_version)) { android_ndk_version = "r26d" }\nif (!defined(android_ndk_root)) { android_ndk_root = "\/\/third_party\/android_toolchain\/ndk" }\n/' "$v8_dir/build/config/android/BUILD.gn"
            fi
        fi

        if [ -f "$v8_dir/v8/tools/BUILD.gn" ]; then
            python3 -c "import re; p = '$v8_dir/v8/tools/BUILD.gn'; s = open(p).read(); open(p, 'w').write(re.sub(r'group\(\"v8_android_test_runner_deps\"\)\s*\{[\s\S]*?(?=\ngroup\(\"v8_testrunner\"\))', 'group(\"v8_android_test_runner_deps\") {\n  testonly = true\n}\n', s))" || true
        fi

        mkdir -p "$v8_dir/build/android/pylib/results/presentation" "$v8_dir/build/android/test_wrapper"
        touch "$v8_dir/build/android/pylib/results/presentation/test_results_presentation.pydeps" \
              "$v8_dir/build/android/devil_chromium.pydeps" \
              "$v8_dir/build/android/apk_operations.pydeps" \
              "$v8_dir/build/android/test_runner.pydeps" \
              "$v8_dir/build/android/test_wrapper/logdog_wrapper.pydeps" \
              "$v8_dir/build/android/resource_sizes.pydeps"

        mkdir -p "$v8_dir/build/linux"
        if [ -d "$HOME/.cache/debian_bullseye_amd64-sysroot" ]; then
            cp -a "$HOME/.cache/debian_bullseye_amd64-sysroot" "$v8_dir/build/linux/"
        else
            python3 "$v8_dir/build/linux/sysroot_scripts/install-sysroot.py" --arch=amd64 || true
            if [ -d "$v8_dir/build/linux/debian_bullseye_amd64-sysroot" ]; then
                mkdir -p "$HOME/.cache"
                cp -a "$v8_dir/build/linux/debian_bullseye_amd64-sysroot" "$HOME/.cache/"
            fi
        fi
    done
fi

# ------------------------------------------------------------------------------
# 8. Build Obscura Native Library (.so)
# ------------------------------------------------------------------------------
echo "🔨 Compiling libobscura_android.so..."
mkdir -p "${WORKSPACE_ROOT}/android/app/src/main/jniLibs"

if [ -s "$ARCHIVE_FILE" ] && [ -s "$BINDING_FILE" ]; then
    echo "⚡ Using prebuilt V8 archive: $ARCHIVE_FILE"
    export RUSTY_V8_ARCHIVE="$ARCHIVE_FILE"
    export RUSTY_V8_SRC_BINDING_PATH="$BINDING_FILE"
    unset V8_FROM_SOURCE || true
else
    export V8_FROM_SOURCE=1
fi

export NINJA_STATUS="[%f/%t (%p)] "

cargo ndk -t arm64-v8a -o "${WORKSPACE_ROOT}/android/app/src/main/jniLibs" build --release -p obscura-android -vv

SO_PATH="${WORKSPACE_ROOT}/android/app/src/main/jniLibs/arm64-v8a/libobscura_android.so"
if [ ! -f "$SO_PATH" ]; then
    echo "❌ Build failed: $SO_PATH was not generated!"
    exit 1
fi
echo "✅ Native library compiled successfully: $(ls -lh "$SO_PATH" | awk '{print $5, $9}')"

# Cache V8 archive if built from source
if [ ! -s "$ARCHIVE_FILE" ]; then
    V8_LIB="$(find target/ -name "librusty_v8.a" 2>/dev/null | head -n 1 || true)"
    V8_BIND="$(find target/ -name "src_binding.rs" 2>/dev/null | head -n 1 || true)"
    if [ -n "$V8_LIB" ] && [ -f "$V8_LIB" ]; then
        echo "Caching V8 static library to $ARCHIVE_FILE..."
        gzip -9 -c "$V8_LIB" > "$ARCHIVE_FILE"
    fi
    if [ -n "$V8_BIND" ] && [ -f "$V8_BIND" ]; then
        cp "$V8_BIND" "$BINDING_FILE"
    fi
fi

# Save to persistent storage (Google Drive / custom dir) so future Colab runs finish in seconds!
if [ -n "$PERSISTENT_CACHE_DIR" ]; then
    echo "💾 Backing up compiled V8 and bindings to persistent cache ($PERSISTENT_CACHE_DIR)..."
    if [ -s "$ARCHIVE_FILE" ]; then
        cp "$ARCHIVE_FILE" "$PERSISTENT_CACHE_DIR/librusty_v8_release_aarch64-linux-android.a.gz" 2>/dev/null || true
    fi
    if [ -s "$BINDING_FILE" ]; then
        cp "$BINDING_FILE" "$PERSISTENT_CACHE_DIR/src_binding_release_aarch64-linux-android.rs" 2>/dev/null || true
    fi
    if command -v sccache >/dev/null 2>&1; then
        sccache --show-stats || true
    fi
    echo "✅ Cache saved to Drive. Future builds will skip V8 compilation completely!"
fi

# ------------------------------------------------------------------------------
# 9. Build Android APK
# ------------------------------------------------------------------------------
echo "📦 Building Android APK via Gradle..."
cd "${WORKSPACE_ROOT}/android"
chmod +x gradlew

# Build release APK (fallback to assembleDebug if release fails)
./gradlew assembleRelease --stacktrace || ./gradlew assembleDebug --stacktrace

cd "${WORKSPACE_ROOT}"
mkdir -p dist

APK_FILE=$(find android/app/build/outputs/apk/release/ -name "*.apk" 2>/dev/null | head -n 1 || true)
if [ -z "$APK_FILE" ]; then
    APK_FILE=$(find android/app/build/outputs/apk/debug/ -name "*.apk" 2>/dev/null | head -n 1 || true)
fi
if [ -z "$APK_FILE" ]; then
    APK_FILE=$(find android/app/build/outputs/apk/ -name "*.apk" 2>/dev/null | head -n 1 || true)
fi

if [ -n "$APK_FILE" ] && [ -f "$APK_FILE" ]; then
    DEST_APK="${WORKSPACE_ROOT}/dist/obscura-android-arm64-v8a.apk"
    cp "$APK_FILE" "$DEST_APK"
    echo ""
    echo "=========================================================="
    echo "🎉 BUILD SUCCESSFUL!"
    echo "=========================================================="
    echo "📱 APK Path: ${DEST_APK}"
    echo "📊 Size:     $(ls -lh "${DEST_APK}" | awk '{print $5}')"
    echo "=========================================================="
else
    echo "❌ Error: No APK found in build outputs!"
    exit 1
fi

