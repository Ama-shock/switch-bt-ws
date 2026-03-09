//! build.rs — BTStack C ライブラリと btkeyLib を静的ライブラリとしてコンパイルする。
//!
//! Windows の場合 :  BTStack 全ソース + btkeyLib + btstack_platform.c をコンパイル
//! 非 Windows の場合: スタブをコンパイルしてクレートを開発・CI 環境でもビルド可能にする
//!
//! BTStack ルートの解決順序（Windows ビルド時）:
//!   1. 環境変数 BTSTACK_ROOT が設定されていればそれを使用
//!      （Docker コンテナ内では /btstack/windows を指定）
//!   2. 未設定の場合は ../windows にフォールバック
//!      （ローカル開発環境向け）

use std::path::{Path, PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os != "windows" {
        println!("cargo:warning=BTStack 統合は Windows 専用です。スタブをコンパイルします。");
        compile_stub();
        return;
    }

    compile_btstack();
}

// ---------------------------------------------------------------------------
// BTStack フルビルド（Windows 専用）
// ---------------------------------------------------------------------------

fn compile_btstack() {
    // BTSTACK_ROOT 環境変数 → フォールバック ../windows
    let btstack_root = if let Ok(root) = std::env::var("BTSTACK_ROOT") {
        PathBuf::from(root)
    } else {
        PathBuf::from("../windows")
    };

    println!("cargo:rerun-if-env-changed=BTSTACK_ROOT");

    if !btstack_root.exists() {
        panic!(
            "BTStack ルートが見つかりません: {:?}\n\
             環境変数 BTSTACK_ROOT を設定するか、Cargo.toml から見て ../windows に \
             mizuyoukanao/btstack のクローンが必要です。\n\
             Docker でビルドする場合は Dockerfile を使用してください。",
            btstack_root.canonicalize().unwrap_or(btstack_root)
        );
    }

    let mut build = cc::Build::new();

    // -----------------------------------------------------------------------
    // インクルードパス
    // -----------------------------------------------------------------------
    build
        // BTStack コアヘッダー
        .include(btstack_root.join("src"))
        .include(btstack_root.join("src/ble"))
        .include(btstack_root.join("src/classic"))
        // プラットフォーム
        .include(btstack_root.join("platform/windows"))
        .include(btstack_root.join("platform/posix"))
        .include(btstack_root.join("platform/embedded"))
        // サードパーティ
        .include(btstack_root.join("3rd-party/bluedroid/decoder/include"))
        .include(btstack_root.join("3rd-party/bluedroid/encoder/include"))
        .include(btstack_root.join("3rd-party/micro-ecc"))
        .include(btstack_root.join("3rd-party/lc3-google/include"))
        .include(btstack_root.join("3rd-party/md5"))
        .include(btstack_root.join("3rd-party/hxcmod-player"))
        .include(btstack_root.join("3rd-party/rijndael"))
        .include(btstack_root.join("3rd-party/yxml"))
        // チップセット
        .include(btstack_root.join("chipset/zephyr"))
        // btstack_config.h が格納されているポートディレクトリ
        .include(btstack_root.join("port/windows-winusb"));

    // -----------------------------------------------------------------------
    // ソースファイル（グロブパターン）
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

    // 不要なファイルを除外する
    // - le_device_db_memory.c  : TLV 版を使うため不要
    // - btstack_audio_portaudio.c : PortAudio 依存のため除外
    // - sco_demo_util.c        : デモ用音声処理、btkeyLib には不要
    let exclude: &[&str] = &[
        "le_device_db_memory.c",
        "btstack_audio_portaudio.c",
        "sco_demo_util.c",
    ];

    for pattern in glob_patterns {
        let full = btstack_root.join(pattern);
        let pattern_str = full.to_string_lossy();

        for entry in glob::glob(&pattern_str).expect("グロブパターンエラー") {
            let path = match entry {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("cargo:warning=グロブエラー: {e}");
                    continue;
                }
            };
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !exclude.contains(&name) {
                add_c_file(&mut build, &path);
            }
        }
    }

    // -----------------------------------------------------------------------
    // アプリケーション固有の C ファイル
    // -----------------------------------------------------------------------

    // Pro Controller エミュレーション本体
    add_c_file(&mut build, &btstack_root.join("example/btkeyLib.c"));

    // プラットフォームラッパー
    // port/windows-winusb/main.c の改変版。
    // main() を btstack_platform_run() にリネームして Rust の main() との衝突を回避。
    add_c_file(&mut build, Path::new("csrc/btstack_platform.c"));

    // -----------------------------------------------------------------------
    // コンパイル実行
    // -----------------------------------------------------------------------
    build.compile("btstack_gamepad");

    // -----------------------------------------------------------------------
    // UAC マニフェスト埋め込み（管理者権限を要求する）
    // -----------------------------------------------------------------------
    compile_manifest();

    // -----------------------------------------------------------------------
    // リンカーフラグ（BTStack WinUSB が必要とする Windows 標準ライブラリ）
    // -----------------------------------------------------------------------
    println!("cargo:rustc-link-lib=winusb");
    println!("cargo:rustc-link-lib=setupapi");

    // -----------------------------------------------------------------------
    // 変更検知トリガー
    // -----------------------------------------------------------------------
    println!("cargo:rerun-if-changed=csrc/btstack_platform.c");
    println!("cargo:rerun-if-changed=csrc/btstack_stub.c");
    println!("cargo:rerun-if-changed=csrc/app.rc");
    println!("cargo:rerun-if-changed=csrc/app.manifest");
}

fn add_c_file(build: &mut cc::Build, path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    build.file(path);
}

// ---------------------------------------------------------------------------
// UAC マニフェスト埋め込み（Windows クロスコンパイル用）
// ---------------------------------------------------------------------------

/// windres (mingw) で .rc → .o にコンパイルし、リンクさせる。
/// これにより exe に管理者権限要求のマニフェストが埋め込まれる。
fn compile_manifest() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_file = Path::new("csrc/app.rc");
    let obj_file = out_dir.join("app.res.o");

    let status = std::process::Command::new("x86_64-w64-mingw32-windres")
        .args([
            "--input",
            &rc_file.to_string_lossy(),
            "--output",
            &obj_file.to_string_lossy(),
            "--output-format=coff",
        ])
        .status()
        .expect("x86_64-w64-mingw32-windres の実行に失敗しました");

    if !status.success() {
        panic!("windres によるマニフェストコンパイルに失敗しました");
    }

    // リンカにオブジェクトファイルを渡す
    println!("cargo:rustc-link-arg={}", obj_file.display());
}

// ---------------------------------------------------------------------------
// スタブビルド（非 Windows — IDE / CI 用）
// ---------------------------------------------------------------------------

fn compile_stub() {
    cc::Build::new()
        .file("csrc/btstack_stub.c")
        .compile("btstack_gamepad");
    println!("cargo:rerun-if-changed=csrc/btstack_stub.c");
}
