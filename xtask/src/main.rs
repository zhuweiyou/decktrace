//! 开发任务入口：`cargo xtask package-macos`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    match command.as_deref() {
        Some("package-macos") => {
            let sign = parse_sign(&mut args)?;
            package_macos(sign.as_deref())
        }
        Some("install-macos") => {
            let mut sign = None;
            let mut open = true;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--sign" => sign = Some(args.next().context("--sign 后面缺证书名称")?),
                    "--no-open" => open = false,
                    other => bail!("未知参数：{other}"),
                }
            }
            package_macos(sign.as_deref())?;
            install_macos(open)
        }
        Some("icon") => regenerate_icon(),
        Some("package-android") => {
            // 默认 .so 用 release + assembleDebug；--release 让 .so 与 APK 都走 release。
            let mut release_apk = false;
            let mut gradle = true;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--release" => release_apk = true,
                    "--skip-gradle" | "--no-gradle" => gradle = false,
                    other => bail!("未知参数：{other}"),
                }
            }
            package_android(release_apk, gradle)
        }
        Some("package-windows") => package_windows(),
        Some("help") | None => {
            println!(
                "用法：\n  cargo xtask package-macos [--sign <证书名>]\n  cargo xtask install-macos  [--sign <证书名>] [--no-open]\n  cargo xtask icon\n  cargo xtask package-android [--release] [--skip-gradle]\n  cargo xtask package-windows\n\n签名：不传 --sign 时优先用本机 decktrace-dev 证书，找不到再退回 ad-hoc。\nad-hoc 每次构建签名都会变，macOS 会当成新应用，屏幕录制权限要重新授权。\n\npackage-android：用 cargo-ndk 编 libjni_bridge.so（arm64-v8a，release）到 android/app/src/main/jniLibs。\n默认跑 gradlew assembleDebug（release .so + debug APK）；--release 则 assembleRelease（.so 与 APK 都 release）。\n--skip-gradle 只出 .so。"
            );
            Ok(())
        }
        Some(other) => bail!(
            "未知 xtask：{other}（可用：package-macos / install-macos / icon / package-android / package-windows）"
        ),
    }
}

fn parse_sign(args: &mut std::iter::Skip<std::env::Args>) -> Result<Option<String>> {
    let mut sign = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sign" => sign = Some(args.next().context("--sign 后面缺证书名称")?),
            other => bail!("未知参数：{other}"),
        }
    }
    Ok(sign)
}

/// 本机装了固定自签名证书就默认用它，TCC 授权才能跨构建保留。
fn default_sign_identity() -> Option<String> {
    if let Ok(identity) = std::env::var("DECKTRACE_SIGN_IDENTITY") {
        if !identity.trim().is_empty() {
            return Some(identity);
        }
    }
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains("\"decktrace-dev\"") {
        return Some("decktrace-dev".to_string());
    }
    None
}

fn install_macos(should_open: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("install-macos 只能在 macOS 上运行");
    }
    let app = workspace_root().join("dist").join(APP_BUNDLE);
    if !app.is_dir() {
        bail!("找不到打包产物：{}", app.display());
    }
    let dest = std::path::Path::new("/Applications").join(APP_BUNDLE);

    // 先退出运行中的实例，否则替换会失败；顺手清掉旧的英文名安装。
    let _ = Command::new("pkill")
        .args(["-f", "decktrace.app"])
        .status();
    std::thread::sleep(Duration::from_millis(600));
    for stale in [dest.as_path(), Path::new("/Applications/decktrace.app")] {
        if stale.exists() {
            fs::remove_dir_all(stale)
                .with_context(|| format!("删除旧安装失败：{}", stale.display()))?;
        }
    }
    run(Command::new("ditto").arg(&app).arg(&dest))?;
    run(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&dest))?;
    if should_open {
        run(Command::new("open").arg(&dest))?;
    }
    println!("{}", dest.display());
    Ok(())
}

/// 从 assets/AppIcon.png 重新生成 AppIcon.icns 与 AppIcon128.png。
fn regenerate_icon() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("icon 依赖 macOS 的 sips/iconutil");
    }
    let root = workspace_root();
    let source = root.join("crates/app/assets/AppIcon.png");
    if !source.is_file() {
        bail!("缺少源图标：{}", source.display());
    }
    let iconset = root.join("target/AppIcon.iconset");
    if iconset.exists() {
        fs::remove_dir_all(&iconset)?;
    }
    fs::create_dir_all(&iconset)?;
    let entries = [
        (16u32, 16u32),
        (32, 16),
        (32, 32),
        (64, 32),
        (128, 128),
        (256, 128),
        (256, 256),
        (512, 256),
        (512, 512),
        (1024, 512),
    ];
    for (pixels, base) in entries {
        let name = if pixels == base {
            format!("icon_{base}x{base}.png")
        } else {
            format!("icon_{base}x{base}@2x.png")
        };
        run(Command::new("sips")
            .args(["-z", &pixels.to_string(), &pixels.to_string()])
            .arg(&source)
            .arg("--out")
            .arg(iconset.join(&name)))?;
    }
    run(Command::new("iconutil")
        .args(["-c", "icns"])
        .arg(&iconset)
        .arg("-o")
        .arg(root.join("crates/app/assets/AppIcon.icns")))?;
    run(Command::new("sips")
        .args(["-z", "128", "128"])
        .arg(&source)
        .arg("--out")
        .arg(root.join("crates/app/assets/AppIcon128.png")))?;
    println!("已从 AppIcon.png 重新生成 AppIcon.icns / AppIcon128.png");
    Ok(())
}

/// 打包产物与安装后的应用名。Finder 里显示的就是这个文件名。
const APP_BUNDLE: &str = "decktrace.app";

/// 安卓只保留一个 ABI（当前 APK 的 abiFilters 也是 arm64-v8a）。
const ANDROID_ABI: &str = "arm64-v8a";
const ANDROID_RUST_TARGET: &str = "aarch64-linux-android";

/// 编译 JNI 的 cdylib 到 jniLibs，再（可选）打 APK。
///
/// - 默认：`.so` 用 release 编译 + `assembleDebug`（可安装的调试 APK）。
/// - `release_apk = true`（`--release`）：`.so` 与 APK 都走 release（`assembleRelease`）。
///
/// 产物：`android/app/src/main/jniLibs/arm64-v8a/libjni_bridge.so`
/// APK：debug → `app/build/outputs/apk/debug/app-debug.apk`
///      release → `app/build/outputs/apk/release/app-release*.apk`（未签名时是 *-unsigned.apk）
fn package_android(release_apk: bool, run_gradle: bool) -> Result<()> {
    let root = workspace_root();
    let android_dir = root.join("android");
    let jni_libs = android_dir.join("app/src/main/jniLibs");
    if !android_dir.is_dir() {
        bail!("找不到 android 工程目录：{}", android_dir.display());
    }

    let cargo_bin = cargo();

    // cargo-ndk 是 cargo 子命令，缺失时给出安装提示而不是难懂的报错。
    let ndk_ok = Command::new(&cargo_bin)
        .args(["ndk", "--version"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !ndk_ok {
        bail!(
            "未找到 cargo-ndk。请先安装：\n  cargo install cargo-ndk\n并安装安卓 Rust target：\n  rustup target add {ANDROID_RUST_TARGET}\n然后设置 ANDROID_NDK_HOME（或确保 SDK 内含 NDK）。"
        );
    }

    // 软检查 target，缺失只提示不阻断（cargo ndk 会给出更明确的错误）。
    if let Ok(out) = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        let installed = String::from_utf8_lossy(&out.stdout);
        if !installed.lines().any(|line| line.trim() == ANDROID_RUST_TARGET) {
            eprintln!(
                "提示：未检测到 Rust target {ANDROID_RUST_TARGET}，如构建失败请先执行：\n  rustup target add {ANDROID_RUST_TARGET}"
            );
        }
    }

    fs::create_dir_all(&jni_libs)
        .with_context(|| format!("创建 jniLibs 目录失败：{}", jni_libs.display()))?;

    // .so 始终用 release 编译（debug 识别慢一个数量级，出包没有意义）。
    println!("[1/2] cargo ndk 编译 libjni_bridge.so（{ANDROID_ABI}，release）…");
    let mut ndk = Command::new(&cargo_bin);
    ndk.args(["ndk", "-t", ANDROID_ABI, "-o"])
        .arg(&jni_libs)
        .arg("build")
        .arg("--release")
        .args(["-p", "jni_bridge"])
        .current_dir(&root);
    run(&mut ndk)?;

    let so = jni_libs.join(ANDROID_ABI).join("libjni_bridge.so");
    if !so.is_file() {
        bail!("构建完成但未找到 .so：{}", so.display());
    }
    println!("      → {}", so.display());

    let (task, apk_rel) = if release_apk {
        ("assembleRelease", "app/build/outputs/apk/release")
    } else {
        ("assembleDebug", "app/build/outputs/apk/debug")
    };

    if run_gradle {
        println!("[2/2] gradlew {task} …");
        let status = gradle_command(&android_dir, task)
            .status()
            .context("执行 gradlew 失败（需要 JDK / ANDROID_HOME）")?;
        if !status.success() {
            bail!("gradlew {task} 失败（{status}）");
        }
        let apk_dir = android_dir.join(apk_rel);
        let apk = fs::read_dir(&apk_dir).ok().and_then(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .find(|path| path.extension().is_some_and(|ext| ext == "apk"))
        });
        match apk {
            Some(path) => println!("      → {}", path.display()),
            None => eprintln!("提示：未在 {} 找到 .apk（构建可能未产出）", apk_dir.display()),
        }
    } else {
        println!("[2/2] 已跳过 gradle（--skip-gradle）");
    }

    println!("完成。");
    Ok(())
}

/// 调用仓库自带的 gradle wrapper：Windows 用 `gradlew.bat`，其它平台用 `sh gradlew`
/// （避免 git 在 Windows 上不保留可执行位导致的 permission denied）。
fn gradle_command(android_dir: &Path, task: &str) -> Command {
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "gradlew.bat", task]);
        cmd
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut cmd = Command::new("sh");
        cmd.args(["gradlew", task]);
        cmd
    };
    cmd.current_dir(android_dir);
    cmd
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask 必须位于 workspace 根目录下")
        .to_path_buf()
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("执行命令失败：{command:?}"))?;
    if !status.success() {
        bail!("命令失败（{status}）：{command:?}");
    }
    Ok(())
}

fn package_macos(sign_identity: Option<&str>) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("package-macos 只能在 macOS 上运行");
    }

    let root = workspace_root();
    run(Command::new(cargo())
        .args(["build", "--release", "-p", "app"])
        .current_dir(&root))?;

    let version = package_version(&root)?;
    let app = root.join("dist").join(APP_BUNDLE);
    if app.exists() {
        fs::remove_dir_all(&app).with_context(|| format!("删除旧产物失败：{}", app.display()))?;
    }
    let macos_dir = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    let assets_root = resources.join("assets");
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&assets_root)?;

    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let binary = target_dir.join("release/decktrace");
    if !binary.is_file() {
        bail!("找不到打包二进制：{}", binary.display());
    }
    fs::copy(&binary, macos_dir.join("decktrace"))
        .with_context(|| format!("拷贝二进制失败：{}", binary.display()))?;
    fs::copy(
        root.join("crates/app/assets/AppIcon.icns"),
        resources.join("AppIcon.icns"),
    )
    .context("拷贝 AppIcon.icns 失败")?;

    // 游戏 crate 约定：crates/<game>/src/profile.rs 且 <game> == GameProfile.id。
    let mut games = Vec::new();
    for entry in fs::read_dir(root.join("crates")).context("读取 crates 目录失败")? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !path.join("src/profile.rs").is_file() {
            continue;
        }
        let game = name;
        let assets = path.join("assets");
        if !assets.is_dir() {
            bail!("游戏 {game} 缺少资源目录：{}", assets.display());
        }
        games.push((game.to_string(), assets));
    }
    games.sort_by(|left, right| left.0.cmp(&right.0));
    for (game, source) in games {
        let dest = assets_root.join(&game);
        if dest.exists() {
            bail!("重复的游戏资源目录：{game}");
        }
        copy_dir(&source, &dest)?;
    }

    // GUI 牌面是应用资源，所有游戏共用。
    let app_cards = root.join("crates/app/assets/gui/cards");
    if !app_cards.is_dir() {
        bail!("缺少 GUI 牌面素材：{}", app_cards.display());
    }
    copy_dir(&app_cards, &assets_root.join("cards"))?;

    fs::write(app.join("Contents/Info.plist"), info_plist(&version))
        .context("写入 Info.plist 失败")?;
    fs::write(app.join("Contents/PkgInfo"), "APPL????").context("写入 PkgInfo 失败")?;

    // 自定义证书签名时 TCC 身份稳定，重建后不用重新授权；否则退回 ad-hoc。
    let identity = sign_identity
        .map(str::to_string)
        .or_else(default_sign_identity)
        .unwrap_or_else(|| "-".to_string());
    let identity = identity.as_str();
    let status = Command::new("codesign")
        .args(["--force", "--deep", "--sign", identity])
        .arg(&app)
        .status();
    if !status.map(|status| status.success()).unwrap_or(false) {
        bail!("codesign 失败（{identity}）");
    }
    run(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app))?;

    println!("{}", app.display());
    Ok(())
}

fn package_windows() -> Result<()> {
    if !cfg!(target_os = "windows") {
        bail!("package-windows 只能在 Windows 上运行");
    }
    let root = workspace_root();
    run(Command::new(cargo())
        .args(["build", "--release", "-p", "app"])
        .current_dir(&root))?;

    let out = root.join("dist").join("decktrace");
    if out.exists() {
        fs::remove_dir_all(&out)
            .with_context(|| format!("删除旧产物失败：{}", out.display()))?;
    }
    let assets = out.join("assets");
    fs::create_dir_all(&assets)?;

    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let exe = target_dir.join("release").join("decktrace.exe");
    if !exe.is_file() {
        bail!("找不到打包二进制：{}", exe.display());
    }
    fs::copy(&exe, out.join("decktrace.exe")).context("拷贝 exe 失败")?;

    // 游戏 crate 约定：crates/<game>/src/profile.rs 且 <game> == GameProfile.id。
    for entry in fs::read_dir(root.join("crates")).context("读取 crates 目录失败")? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !path.join("src/profile.rs").is_file() {
            continue;
        }
        let source = path.join("assets");
        if !source.is_dir() {
            bail!("游戏 {name} 缺少资源目录：{}", source.display());
        }
        copy_dir(&source, &assets.join(name))?;
    }

    // GUI 牌面是应用资源，所有游戏共用。
    let app_cards = root.join("crates/app/assets/gui/cards");
    if !app_cards.is_dir() {
        bail!("缺少 GUI 牌面素材：{}", app_cards.display());
    }
    copy_dir(&app_cards, &assets.join("cards"))?;

    println!("{}", out.display());
    Ok(())
}

fn package_version(root: &Path) -> Result<String> {
    let output = Command::new(cargo())
        .args(["pkgid", "-p", "app"])
        .current_dir(root)
        .output()
        .context("执行 cargo pkgid 失败")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.rsplit('#').next().unwrap_or("").trim();
    if version.is_empty() {
        bail!("无法确定 decktrace 版本");
    }
    Ok(version.to_string())
}

fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("创建目录失败：{}", dest.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("读取目录失败：{}", source.display()))?
    {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>zh_CN</string>
    <key>CFBundleExecutable</key>
    <string>decktrace</string>
    <key>CFBundleIdentifier</key>
    <string>com.zhuweiyou.decktrace</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>decktrace</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleDisplayName</key>
    <string>decktrace</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.3</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.games</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSScreenCaptureUsageDescription</key>
    <string>需要截取斗地主窗口来识别牌面并显示剩余牌。</string>
</dict>
</plist>
"#
    )
}
