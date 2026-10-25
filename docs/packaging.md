# 打包与发布

## 命令

```bash
cargo xtask install-macos   # 日常用：打包 + 签名 + 安装到 /Applications
cargo xtask package-macos   # 只生成 dist/斗地主记牌器.app
cargo xtask icon            # 由 AppIcon.png 重新生成 icns 和 128px 图标
```

`cargo xtask` 通过 `.cargo/config.toml` 的 alias 调用 `xtask` crate，
打包逻辑全是 Rust，不要恢复 shell 脚本。

产物：`dist/斗地主记牌器.app`，安装后是 `/Applications/斗地主记牌器.app`，
bundle id `com.zhuweiyou.decktrace`，可执行文件名 `decktrace`。

## install-macos 做什么

1. 调用 `package-macos`
2. 退出正在运行的实例
3. 删除旧安装（包括历史英文名 `decktrace.app`）
4. `ditto` 拷贝到 `/Applications/斗地主记牌器.app`
5. `codesign --verify` 校验签名
6. `open` 启动

加 `--no-open` 只安装不启动。

## package-macos 做什么

1. `cargo build --release -p decktrace`
2. 重建 `dist/斗地主记牌器.app`
3. 拷贝二进制到 `Contents/MacOS/decktrace`
4. 拷贝 `crates/decktrace/assets/AppIcon.icns` 到 `Contents/Resources/`
5. 扫描 `crates/*/src/profile.rs`，识别游戏 crate
6. 拷贝 `crates/<game>/assets` 到 `Contents/Resources/assets/<game>`
7. 拷贝 `crates/decktrace/assets/cards` 到 `Contents/Resources/assets/cards`
8. 写 `Contents/Info.plist` 和 `Contents/PkgInfo`
9. 用固定证书签名，并立即 `codesign --verify` 校验

> `Info.plist` 必须在 `Contents/` 下。写到 `Contents/Resources/` 会被
> codesign 判定为 `bundle format unrecognized`。

## 资源约定

- 游戏 crate 目录名必须等于 `GameProfile.id`：
  - `crates/jjddz` → `id = "jjddz"`
  - `crates/hlddz` → `id = "hlddz"`
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
  jjddz/templates
  hlddz/templates
  cards/
```

## App 图标

- 源文件：`crates/decktrace/assets/AppIcon.png`（1024×1024）
- 生成物：`AppIcon.icns`、`AppIcon128.png`（Dock/窗口图标）
- 重新生成：`cargo xtask icon`（依赖 macOS 的 `sips`、`iconutil`）
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

## 签名与屏幕录制权限

macOS 的屏幕录制授权绑定在**代码签名**上：

- 默认用本机 `decktrace-dev` 自签名证书（xtask 自动探测，`--sign <名称>` 可覆盖）。
- 找不到证书时退回 ad-hoc；ad-hoc 每次构建签名都会变，系统会当成新应用，
  屏幕录制要重新授权。
- 证书只需在钥匙串里导入一次；之后 `cargo xtask install-macos` 直接复用，
  不会再弹授权。
- 环境变量 `DECKTRACE_SIGN_IDENTITY` 也可以指定证书。

自签名证书的准备（只做一次）：

1. 用 openssl 生成带 `codeSigning` 用途的自签名证书
2. `security import identity.p12` 导入登录钥匙串
3. `security add-trusted-cert` 信任证书
4. `security find-identity -v -p codesigning` 能看到 `decktrace-dev` 即可

日常开发也可以直接 `cargo run --release -p decktrace`：子进程继承终端/IDE
的权限，不会触发授权弹窗。

对外分发仍需 Developer ID Application 证书、Hardened Runtime 和公证。

## Windows

- 当前 `crates/capture/src/windows.rs` 是实验性的 GDI `PrintWindow`/`BitBlt` 后端。
- 已通过 Windows GNU target 的 `cargo check`，未在真实 Windows 游戏窗口验证。
- 正式目标、WGC 实现步骤和验证矩阵见 [Windows 采集路线](windows.md)。
- 后续增加 `cargo xtask package-windows`；当前只支持 macOS。
- 不要把平台判断写进 `Session` 或游戏层。
