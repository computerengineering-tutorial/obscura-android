#!/usr/bin/env python3
"""
Patch rusty_v8 build.rs for Android cross-compilation:
- Ensures V8_FROM_SOURCE only applies to the cross-compilation target (Android),
  so host build dependencies (obscura-js build.rs) use fast prebuilt binaries.
- Option 1 (Rebuild mode): Configures Clang bindgen with Android target & NDK sysroot.
- Option 2 (Fast mode): Bypasses bindgen by using pre-generated binary-compatible bindings.
"""

import sys
import os
import re
from pathlib import Path


def patch_v8_build_rs(build_rs_path: str):
    p = Path(build_rs_path)
    if not p.is_file():
        print(f"⚠️ {p} not found, skipping.")
        return False

    content = p.read_text(encoding="utf-8")
    original = content

    # 1. Patch static_lib_url for Android (only override for target, not host)
    old_static_url = '''fn static_lib_url() -> String {
  if let Ok(custom_archive) = env::var("RUSTY_V8_ARCHIVE") {
    return custom_archive;
  }'''
    new_static_url = '''fn static_lib_url() -> String {
  let target = env::var("TARGET").unwrap_or_default();
  let host = env::var("HOST").unwrap_or_default();
  if target != host || target.contains("android") {
    if let Ok(custom_archive) = env::var("RUSTY_V8_ARCHIVE") {
      return custom_archive;
    }
  }'''
    if old_static_url in content:
        content = content.replace(old_static_url, new_static_url, 1)

    # 2. Patch print_prebuilt_src_binding_path for Android
    old_print_binding = '''fn print_prebuilt_src_binding_path() {
  if let Ok(binding) = env::var("RUSTY_V8_SRC_BINDING_PATH") {
    println!("cargo:rustc-env=RUSTY_V8_SRC_BINDING_PATH={binding}");
    return;
  }'''
    new_print_binding = '''fn print_prebuilt_src_binding_path() {
  let target = env::var("TARGET").unwrap_or_default();
  let host = env::var("HOST").unwrap_or_default();
  if target != host || target.contains("android") {
    if let Ok(binding) = env::var("RUSTY_V8_SRC_BINDING_PATH") {
      println!("cargo:rustc-env=RUSTY_V8_SRC_BINDING_PATH={binding}");
      return;
    }
  }'''
    if old_print_binding in content:
        content = content.replace(old_print_binding, new_print_binding, 1)

    # 3. Patch V8_FROM_SOURCE so host dependencies NEVER build from source
    old_from_source = '  // Build from source\n  if env_bool("V8_FROM_SOURCE") {'
    new_from_source = '''  // Build from source (only for Android or cross-compilation target; host uses prebuilts)
  let target = env::var("TARGET").unwrap_or_default();
  let host = env::var("HOST").unwrap_or_default();
  let is_cross_target = target.contains("android") || target != host;
  if env_bool("V8_FROM_SOURCE") && is_cross_target {'''
    if old_from_source in content:
        content = content.replace(old_from_source, new_from_source, 1)

    # 4. Patch build_binding entry for Option 2 (Pre-generated bindings bypass for Android & Host Linux)
    # Match either original build_binding() or previously patched build_binding()
    new_entry = """fn build_binding() {
  let out_path = build_dir().join("gn_out").join("src_binding.rs");
  let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
  let rebuild_mode = env::var("OBSCURA_REBUILD_V8").unwrap_or_default() == "1";

  // Option 2 (Default): Pre-generated bindings bypass (100% ABI compatible, instant, bypasses bindgen)
  if !rebuild_mode {
    let binding_file = if target_os == "android" {
      Some("src_binding_release_aarch64-unknown-linux-gnu.rs")
    } else if target_os == "linux" {
      if target_arch == "aarch64" {
        Some("src_binding_release_aarch64-unknown-linux-gnu.rs")
      } else if target_arch == "x86_64" {
        Some("src_binding_release_x86_64-unknown-linux-gnu.rs")
      } else {
        None
      }
    } else {
      None
    };

    if let Some(file_name) = binding_file {
      let prebuilt = get_dirs().root.join("gen").join(file_name);
      if prebuilt.exists() {
        println!("⚡ [Option 2] Using pre-generated bindings for {target_os}-{target_arch} (bypassing bindgen)");
        std::fs::create_dir_all(out_path.parent().unwrap()).ok();
        std::fs::copy(&prebuilt, &out_path).expect("Failed to copy prebuilt binding");
        println!("cargo:rustc-env=RUSTY_V8_SRC_BINDING_PATH={}", out_path.display());
        return;
      }
    }
  }"""

    # If already patched with earlier Option 2 pattern, replace it with new generalized pattern
    if '⚡ [Option 2] Using pre-generated aarch64 bindings for Android' in content:
        content = re.sub(
            r'fn build_binding\(\)\s*\{[\s\S]*?(?=  // Bindgen needs Clang 19\+)',
            new_entry + "\n",
            content,
            count=1
        )
    elif "fn build_binding() {" in content and "OBSCURA_REBUILD_V8" not in content:
        content = content.replace("fn build_binding() {", new_entry, 1)

    # 5. Patch build_binding target branch for Option 1 (Full Clang NDK sysroot bindgen)
    old_ios = '  } else if target_os == "ios" {'
    new_android = """  } else if target_os == "android" {
    // Option 1 (Rebuild): Full Clang bindgen with Android NDK sysroot
    println!("⚡ [Option 1] Building bindings from source using Clang and Android NDK sysroot");
    let target_triple = env::var("TARGET").unwrap();
    clang_args.push(format!("--target={target_triple}"));
    let ndk = env::var("ANDROID_NDK_HOME")
      .or_else(|_| env::var("NDK_HOME"))
      .or_else(|_| env::var("ANDROID_NDK_ROOT"))
      .unwrap_or_default();
    if !ndk.is_empty() {
      let sysroot = format!("{ndk}/toolchains/llvm/prebuilt/linux-x86_64/sysroot");
      clang_args.push(format!("--sysroot={sysroot}"));
      clang_args.push(format!("-isystem{sysroot}/usr/include"));
      clang_args.push(format!("-isystem{sysroot}/usr/include/aarch64-linux-android"));
    }
    if let Ok(libclang_path) = env::var("LIBCLANG_PATH") {
      let clang_dir = std::path::PathBuf::from(&libclang_path)
        .parent()
        .unwrap()
        .to_path_buf();
      let clang_bin = clang_dir.join("bin/clang");
      if let Ok(output) = std::process::Command::new(clang_bin).arg("-print-resource-dir").output() {
        let resource_dir = String::from_utf8(output.stdout).unwrap();
        clang_args.push(format!("-isystem{}/include", resource_dir.trim()));
      }
    }
  } else if target_os == "ios" {"""
    if old_ios in content and "Option 1 (Rebuild)" not in content:
        content = content.replace(old_ios, new_android, 1)

    if content != original:
        p.write_text(content, encoding="utf-8")
        print(f"✅ Successfully patched {p} with Option 1 & Option 2 (Target & Host) support.")
        return True
    else:
        print(f"ℹ️ {p} was already up-to-date.")
        return False


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <v8_dir_or_build_rs> [...]")
        sys.exit(1)

    for arg in sys.argv[1:]:
        p = Path(arg)
        if p.is_dir():
            target_file = p / "build.rs"
        else:
            target_file = p
        patch_v8_build_rs(str(target_file))


if __name__ == "__main__":
    main()
