# 打包与发布

## 命令

```bash
cargo xtask package-macos
open dist/decktrace.app
```

`cargo xtask` 通过 `.cargo/config.toml` 的 alias 调用 `xtask` crate。
它替代了 shell 脚本，打包逻辑全是 Rust。

## package-macos 做什么

1. `cargo build --release -p decktrace`
2. 生成 `dist/decktrace.app`
3. 拷贝二进制和 `AppIcon.icns`
4. 扫描 `crates/*/src/profile.rs`，识别游戏 crate
5. 拷贝 `crates/<game>/assets` 到 `Contents/Resources/assets/<game>`
6. 拷贝 `crates/decktrace/assets/cards` 到 `Contents/Resources/assets/cards`
7. 写 `Info.plist`
8. ad-hoc codesign

## 资源约定

- 游戏 crate 目录名必须等于 `GameProfile.id`：
  - `crates/jjddz` → `id = "jjddz"`
  - 未来的 `crates/hlddz` → `id = "hlddz"`
- 每个游戏 crate 必须有 `src/profile.rs` 和 `assets/`
- 资源目录按游戏分开放，不会互相覆盖
- 目标目录重复时 xtask 直接报错

运行时资源查找顺序：

```text
DECKTRACE_ASSETS/<game>      # 识别模板
DECKTRACE_ASSETS/cards       # GUI 牌面（应用共用）
```

打包后：

```text
Contents/Resources/assets/
  <game>/templates
  cards/
```

## App 图标

- 源文件：`crates/decktrace/assets/AppIcon.png`
- icns：`crates/decktrace/assets/AppIcon.icns`
- `Info.plist` 使用 `CFBundleIconFile=AppIcon`

## macOS：Swift runtime

`screencapturekit` 的 Swift 桥接依赖 Swift 运行时：

- `.cargo/config.toml` 给最终二进制加 `-rpath /usr/lib/swift`
- `crates/capture/build.rs` 把 CLT 的 Swift 静态兼容库目录加入 link search

这两处不要删，否则 dyld 会找不到 `libswift_Concurrency.dylib`。

## Info.plist

必须包含：

- `NSScreenCaptureUsageDescription`
- `CFBundleIconFile`
- `LSMinimumSystemVersion`
- bundle id `com.zhuweiyou.decktrace`

版本号由 `cargo pkgid -p decktrace` 动态读取。

## 签名

当前只做 ad-hoc 签名，便于本机 TCC 稳定识别。

正式分发需要：

- Developer ID Application 证书
- Hardened Runtime
- notarization

## Windows

- `crates/capture/src/windows.rs` 当前是可编译占位。
- 窗口枚举和帧流尚未实现。
- 接入 Windows Graphics Capture / PrintWindow 时，只改 `capture` 的 Windows `cfg` 分支。
- 后续增加 `cargo xtask package-windows`，不要把平台判断写进 `Session` 或游戏层。
