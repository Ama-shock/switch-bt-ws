/// build.rs — Compiles BTStack C library and btkeyLib into a static library.
///
/// On Windows:  compiles the full BTStack + btkeyLib + btstack_platform.c
/// On non-Windows: compiles a stub so the crate builds for development.
///
/// Expected layout (relative to this file):
///   ../windows/  — the mizuyoukanao/btstack fork (port/windows-winusb, example/, src/, …)
///   csrc/btstack_platform.c  — our modified main.c (no main() symbol)
///   csrc/btstack_stub.c      — empty stubs for non-Windows builds

use std::path::{Path, PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os != "windows" {
        println!("cargo:warning=BTStack integration is Windows-only; compiling stub.");
        compile_stub();
        return;
    }

    compile_btstack();
}

// ---------------------------------------------------------------------------
// Full BTStack build (Windows only)
// ---------------------------------------------------------------------------

fn compile_btstack() {
    let btstack_root = PathBuf::from("../windows");

    if !btstack_root.exists() {
        panic!(
            "BTStack root not found at {:?}. \
             Expected the mizuyoukanao/btstack clone at ../windows relative to Cargo.toml.",
            btstack_root.canonicalize().unwrap_or(btstack_root)
        );
    }

    let mut build = cc::Build::new();

    // -----------------------------------------------------------------------
    // Include paths
    // -----------------------------------------------------------------------
    build
        // BTStack core headers
        .include(btstack_root.join("src"))
        .include(btstack_root.join("src/ble"))
        .include(btstack_root.join("src/classic"))
        // Platform
        .include(btstack_root.join("platform/windows"))
        .include(btstack_root.join("platform/posix"))
        .include(btstack_root.join("platform/embedded"))
        // 3rd-party
        .include(btstack_root.join("3rd-party/bluedroid/decoder/include"))
        .include(btstack_root.join("3rd-party/bluedroid/encoder/include"))
        .include(btstack_root.join("3rd-party/micro-ecc"))
        .include(btstack_root.join("3rd-party/lc3-google/include"))
        .include(btstack_root.join("3rd-party/md5"))
        .include(btstack_root.join("3rd-party/hxcmod-player"))
        .include(btstack_root.join("3rd-party/rijndael"))
        .include(btstack_root.join("3rd-party/yxml"))
        // Chipset
        .include(btstack_root.join("chipset/zephyr"))
        // Port config (btstack_config.h lives here)
        .include(btstack_root.join("port/windows-winusb"));

    // -----------------------------------------------------------------------
    // Source file globs
    // -----------------------------------------------------------------------
    let glob_patterns: &[&str] = &[
        "src/*.c",
        "src/classic/*.c",
        "src/ble/*.c",
        "src/ble/gatt-service/*.c",
        "src/mesh/*.c",
        "src/mesh/gatt-service/*.c",
        "3rd-party/bluedroid/encoder/srce/*.c",
        "3rd-party/bluedroid/decoder/srce/*.c",
        "3rd-party/micro-ecc/uECC.c",
        "3rd-party/md5/md5.c",
        "3rd-party/rijndael/rijndael.c",
        "3rd-party/hxcmod-player/*.c",
        "3rd-party/hxcmod-player/mods/*.c",
        "3rd-party/yxml/yxml.c",
        "platform/windows/*.c",
        "platform/posix/wav_util.c",
        "chipset/zephyr/*.c",
    ];

    // Files to exclude (portaudio audio backend, conflicting memory impl, …)
    let exclude: &[&str] = &[
        "le_device_db_memory.c",
        "btstack_audio_portaudio.c",
        // sco_demo_util pulls in audio deps we don't need
        "sco_demo_util.c",
    ];

    for pattern in glob_patterns {
        let full = btstack_root.join(pattern);
        let pattern_str = full.to_string_lossy();

        for entry in glob::glob(&pattern_str).expect("Bad glob pattern") {
            let path = match entry {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("cargo:warning=glob error: {e}");
                    continue;
                }
            };
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if !exclude.contains(&name) {
                add_c_file(&mut build, &path);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Application-specific C files
    // -----------------------------------------------------------------------

    // The Pro Controller emulator (the heart of the project)
    add_c_file(&mut build, &btstack_root.join("example/btkeyLib.c"));

    // Our platform wrapper — replaces port/windows-winusb/main.c so that
    // there is no competing main() symbol when linking with Rust.
    add_c_file(&mut build, Path::new("csrc/btstack_platform.c"));

    // -----------------------------------------------------------------------
    // Compile
    // -----------------------------------------------------------------------
    build.compile("btstack_gamepad");

    // -----------------------------------------------------------------------
    // Linker flags — Windows-specific libraries required by BTStack WinUSB
    // -----------------------------------------------------------------------
    println!("cargo:rustc-link-lib=winusb");
    println!("cargo:rustc-link-lib=setupapi");

    // -----------------------------------------------------------------------
    // Re-run triggers
    // -----------------------------------------------------------------------
    println!("cargo:rerun-if-changed=csrc/btstack_platform.c");
    println!("cargo:rerun-if-changed=csrc/btstack_stub.c");
    println!("cargo:rerun-if-changed=../windows/example/btkeyLib.c");
    println!("cargo:rerun-if-changed=../windows/port/windows-winusb/btstack_config.h");
}

fn add_c_file(build: &mut cc::Build, path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    build.file(path);
}

// ---------------------------------------------------------------------------
// Stub build (non-Windows — lets the crate compile for IDE / CI)
// ---------------------------------------------------------------------------

fn compile_stub() {
    cc::Build::new()
        .file("csrc/btstack_stub.c")
        .compile("btstack_gamepad");
    println!("cargo:rerun-if-changed=csrc/btstack_stub.c");
}
