# 安卓端与 ADB 调试

目标：把记牌器做到手机端——先期用 `adb` 抓手机画面在 Mac 上调试识别，
跑通后打包成纯 Android App：端上采集（MediaProjection）+ 端上识别（Rust 核心），
用悬浮窗在手机顶部显示记牌条。

## 阶段划分

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| 一 | adb 取帧 + 桌面 GUI 调试 | ✅ 已跑通 |
| 二 | Rust 核心编成 `.so`，Kotlin 端上采集识别 | 待做 |
| 三 | 原生悬浮窗（TYPE_APPLICATION_OVERLAY） | 待做 |

adb 只是「取帧器」，它的帧率（单帧 `screencap` 约 2s）和延迟**不代表产品性能**；
产品性能在端上高帧率采集时另测。

## 手机版是独立 crate

手机横屏小程序版和 PC 版布局/字号不同，**单独建 `crates/hlddz-mobile`**，
模板与区域、hand 比例独立标定；改它不影响 PC 版 `hlddz`。
（`GameProfile.id = "hlddz-mobile"`，目录名与 id 一致。）

- 识别实现在 `crates/hlddz-mobile/src/recognition.rs`（由 `hlddz` 复制并按手机分辨率的
  `reference_width` 缩放大小王竖排字母尺寸阈值）。
- 模板在 `crates/hlddz-mobile/assets/templates`，当前与 PC 通用，后续按机型重采。
- 底牌区域、hand 比例按 2780×1264（横屏）实测：底牌中心 y≈0.07、x≈0.47–0.51；
  手牌字形底部 ≈0.776。

## 阶段一用法

```bash
# 1) 确认设备
adb devices -l
adb -s <serial> shell wm size        # 竖屏自然方向，如 1264x2780

# 2) 桌面 GUI 实时驱动（游戏选择 hlddz-mobile）
cargo run --release -p decktrace -- \
    --capture adb --game hlddz-mobile --adb-serial <serial> --fps 2

# 3) 无头调牌（打印每帧快照，不启动 GUI）
cargo run --release -p decktrace --example tracker_probe -- \
    --adb --serial <serial> --frames 40 --dump /tmp/live

# 4) 离线回放（静态单帧，1 帧即入账）
cargo run --release -p decktrace --example tracker_probe -- \
    --replay a.png b.png b.png

# 5) 单帧识别 + 标注图
cargo run --release -p hlddz-mobile --example check -- frame.png
```

参数/环境变量：

- `--capture adb`：选用 adb 后端（默认 `auto` 走本机窗口）。
- `--adb-serial`：多设备时指定序列号；也可用 `DECKTRACE_ADB_SERIAL` / `ANDROID_SERIAL`。
- `--adb-crop x,y,w,h`：截图裁剪区域（截图像素）；也可用 `DECKTRACE_ADB_CROP`。
- `DECKTRACE_ADB_BIN`：adb 可执行路径。

## 已知限制（阶段一）

- adb 帧率低（~0.5fps），`tracker_probe`/GUI 在 adb 模式下强制 `min_hits=1`；
  端上高帧率才用 2 帧确认。
- 慢帧下自动重置（依赖 `absent_seconds`）不可靠，调试多局建议手动重置。
- 底牌字号很小，红桃/方块字形相近，是**重点回归风险点**（见下）。

## 花色小字复核（2780×1264）

曾怀疑底牌 `2♦` 被识别成 `2♥`。放大底牌区核对后确认：该局三张底牌实为
`2♥ A♦ 6♠`，识别器输出完全一致（这局是地主，底牌同时出现在手牌中，
同一张牌在两处出现属正常）。**结论：不是误判**，之前是把小图红桃误看成方块。
小字 D/H 仍列为回归重点，后续多抓样本验证。

## 工具链（已就绪）

- JDK：`openjdk@21`（`/opt/homebrew/opt/openjdk@21`）
- Android SDK：`/opt/homebrew/share/android-commandlinetools`
  （`platforms;android-34`、`build-tools;34.0.0`、`ndk;27.2.12479018`、`platform-tools`）
- Rust 目标：`aarch64-linux-android`、`armv7-linux-androideabi`、`x86_64-linux-android`
- `cargo-ndk 4.1.2`

## 阶段二/三 计划

1. `cargo-ndk` 把 `tracking + hlddz-mobile` 编成 `libdecktrace_core.so`（arm64-v8a）。
2. Kotlin：`MediaProjection` + `VirtualDisplay` + `ImageReader` 取帧，
   JNI 送进 Rust，返回快照 JSON；模板资产打包解压到 `filesDir`。
3. 原生 `WindowManager` 悬浮窗，用 `cards/` 的 54 张 PNG 绘制牌条。
