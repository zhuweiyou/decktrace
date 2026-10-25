//! 开发任务入口：`cargo xtask package-macos`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("package-macos") => package_macos(),
        Some("help") | None => {
            println!("用法：cargo xtask package-macos");
            Ok(())
        }
        Some(other) => bail!("未知 xtask：{other}（可用：package-macos）"),
    }
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

fn package_macos() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("package-macos 只能在 macOS 上运行");
    }

    let root = workspace_root();
    run(Command::new(cargo())
        .args(["build", "--release", "-p", "decktrace"])
        .current_dir(&root))?;

    let version = package_version(&root)?;
    let app = root.join("dist/decktrace.app");
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
        root.join("crates/decktrace/assets/AppIcon.icns"),
        resources.join("AppIcon.icns"),
    )
    .context("拷贝 AppIcon.icns 失败")?;

    // 游戏 crate 约定：crates/decktrace-<game>/src/profile.rs 且 <game> == GameProfile.id。
    let mut games = Vec::new();
    for entry in fs::read_dir(root.join("crates")).context("读取 crates 目录失败")? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(game) = name.strip_prefix("decktrace-") else {
            continue;
        };
        if !path.join("src/profile.rs").is_file() {
            continue;
        }
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

    fs::write(resources.join("Info.plist"), info_plist(&version))
        .context("写入 Info.plist 失败")?;
    fs::write(app.join("Contents/PkgInfo"), "APPL????").context("写入 PkgInfo 失败")?;

    // ad-hoc 签名便于本机 TCC 稳定识别；正式分发仍需 Developer ID + 公证。
    let status = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app)
        .status();
    if !status.map(|status| status.success()).unwrap_or(false) {
        eprintln!("警告：ad-hoc codesign 失败，产物仍已生成");
    }

    println!("{}", app.display());
    Ok(())
}

fn package_version(root: &Path) -> Result<String> {
    let output = Command::new(cargo())
        .args(["pkgid", "-p", "decktrace"])
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
    <string>斗地主记牌器</string>
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
