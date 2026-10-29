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

手机横屏小程序版和 PC 版布局/字号不同，**单独建 `crates/hlddz_mobile`**，
模板与区域、hand 比例独立标定；改它不影响 PC 版 `hlddz`。
（`GameProfile.id = "hlddz_mobile"`，目录名与 id 一致。）

- 识别实现在 `crates/hlddz_mobile/src/recognition.rs`（由 `hlddz` 复制并按手机分辨率的
  `reference_width` 缩放大小王竖排字母尺寸阈值）。
- 模板在 `crates/hlddz_mobile/assets/templates`，当前与 PC 通用，后续按机型重采。
- 底牌区域、hand 比例按 2780×1264（横屏）实测：底牌中心 y≈0.07、x≈0.47–0.51；
  手牌字形底部 ≈0.776。

### 多游戏与王者荣耀斗地主（占位）

- 目前支持两个手机版导航：`hlddz_mobile`（识别/素材已实现）和 `wzryddz`（**基础架子**，识别与模板待补）。
- **手机版不参与 PC 窗口自动检测**：`list_games()` 只返回 PC 游戏（`jjddz`/`hlddz`）。
  PC 上调试手机版必须用 `--capture adb --game hlddz_mobile|wzryddz`；正式只在 APK/JNI 里用。
- `crates/wzryddz` 只有 `GameProfile`（id `wzryddz`）+ 占位识别器（返回空结果），
  `assets/templates` 为空目录；端上选择它能跑通采集链路，但不会识别牌。
- JNI 的 `create(assetsDir, gameId)` 按 `gameId` 动态选择 profile 与识别器；未知 id 返回 0。
- 端上资源按游戏分目录放到 `filesDir/<gameId>/templates`；`wzryddz` 暂无模板，仅建空目录。
  等真实素材就绪后再给 APK 资产做 per-game 命名空间（当前 gradle 仍复用 `hlddz_mobile/assets`）。

## 阶段一用法

```bash
# 1) 确认设备
adb devices -l
adb -s <serial> shell wm size        # 竖屏自然方向，如 1264x2780

# 2) 桌面 GUI 实时驱动（游戏选择 hlddz_mobile）
cargo run --release -p app -- \
    --capture adb --game hlddz_mobile --adb-serial <serial> --fps 2

# 3) 无头调牌（打印每帧快照，不启动 GUI）
cargo run --release -p app --example tracker_probe -- \
    --adb --serial <serial> --frames 40 --dump /tmp/live

# 4) 离线回放（静态单帧，1 帧即入账）
cargo run --release -p app --example tracker_probe -- \
    --replay a.png b.png b.png

# 5) 单帧识别 + 标注图
cargo run --release -p hlddz_mobile --example check -- frame.png
```

参数/环境变量：

- `--capture adb`：选用 adb 后端（默认 `auto` 走本机窗口）。
- `--adb-serial`：多设备时指定序列号；也可用 `DECKTRACE_ADB_SERIAL` / `ANDROID_SERIAL`。
- `--adb-crop x,y,w,h`：截图裁剪区域（截图像素）；也可用 `DECKTRACE_ADB_CROP`。
- `DECKTRACE_ADB_BIN`：adb 可执行路径。

Windows 兼容：`adb` 会自动解析为 `adb.exe`（无需手动加后缀）；`DECKTRACE_ADB_BIN`
指向无扩展名路径时也会自动补 `.exe`。首次运行 `adb` 时 daemon 启动提示若混进 stdout，
会按 PNG 魔数自动剥离，不会导致解码失败。

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

1. `cargo-ndk` 把 `tracking + hlddz_mobile` 编成 `libjni_bridge.so`（arm64-v8a）。
2. Kotlin：`MediaProjection` + `VirtualDisplay` + `ImageReader` 取帧，
   JNI 送进 Rust，返回快照 JSON；模板资产打包解压到 `filesDir`。
3. 原生 `WindowManager` 悬浮窗，用 `cards/` 的 54 张 PNG 绘制牌条。

## 阶段二：真机 APK（已产出）

工程在 `android/`（Java + 原生 View，无 AndroidX）：

- `MainActivity`：竖屏配置页，分两组：① 悬浮窗授权 + 停止；② 选择游戏开始截屏
  （小程序欢乐斗地主 / 王者荣耀斗地主）。
- `CaptureService`：前台服务（`mediaProjection` 类型），`MediaProjection` + `VirtualDisplay`
  + `ImageReader` 取帧 → `NativeBridge.pushFrame` → 刷新悬浮窗。只在横屏采集（竖屏视为不在游戏中，
  释放采集并隐藏悬浮窗；旋转用 `DisplayManager` 监听自适应）。
- `OverlayView`：顶部**单行**记牌条，可拖动；炸弹反色、底牌“底”印；底部一行状态
  `armed / seen / 剩 / det / 底`。加了 `FLAG_SECURE`（录屏里不把自己的牌拍进去）。
- JNI：`crates/jni_bridge` 编 `libjni_bridge.so`（crate-type cdylib，
  lib 名 `jni_bridge`），对外 `create(assetsDir, gameId)/pushFrame/reset/destroy`；
  `create` 按 `gameId`（`hlddz_mobile` / `wzryddz`）动态选择 profile 与识别器，未知 id 返回 0。
- 模板/牌面资源直接复用 `crates/hlddz_mobile/assets` 与 `crates/app/assets/gui`
  （Gradle `sourceSets.assets.srcDirs`）。

构建（推荐用 xtask，一条命令出 .so + APK）：

```bash
# 前置：cargo install cargo-ndk && rustup target add aarch64-linux-android
# 并设置 ANDROID_NDK_HOME / ANDROID_HOME（gradle 还需要 JAVA_HOME）
cargo xtask package-android                 # release .so + assembleDebug（可安装的调试 APK）
cargo xtask package-android --release       # release .so + assembleRelease（APK 也 release）
cargo xtask package-android --skip-gradle   # 只编 .so，APK 自己打
```

产物：

- `.so`：`android/app/src/main/jniLibs/arm64-v8a/libjni_bridge.so`（始终 release 编译）
- debug APK：`android/app/build/outputs/apk/debug/app-debug.apk`
- release APK：`android/app/build/outputs/apk/release/app-release*.apk`（未配置签名时为 `*-unsigned.apk`）

安装：`adb install -r <上面的 apk 路径>`

等价的底层命令（xtask 内部就是这两步，便于排查）：

```bash
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p jni_bridge
cd android && ./gradlew assembleDebug      # --release 时是 assembleRelease
```

> 只保留 `arm64-v8a` 一个 ABI（与 `android/app/build.gradle` 的 `abiFilters` 一致）。
> `android/app/src/main/jniLibs/` 是构建产物，已在 `.gitignore` 里，不会入库。

资源/图标：`android/app/src/main/res/mipmap-*/ic_launcher.png` 由 `crates/app/assets/AppIcon.png` 生成。

## 重要已知问题：ColorOS 屏蔽微信画面

在 OnePlus/ColorOS（PKR110，Android 14）上，用 MediaProjection 采集时系统会提示
「应用内容已屏蔽 · 微信内容对方不可见」，**把微信画面屏蔽成黑屏**，导致识别 `det=0`。
`adb exec-out screencap` 是特权通道，能看到画面，所以容易误判为识别问题。

- 现场可点提示里的「解除屏蔽」放行；但这是系统/微信的隐私保护，不稳定。
- 结论：采集链路已跑通（帧尺寸/stride/送入识别均正常），但**微信小程序这条路线受系统限制**。
- 决策：APK 保持「单应用采集 + 单行记牌条 + 可拖动」，先不做微信特化；
  下一步做**非微信（独立 App / 可正常录屏）的游戏版本**再端到端验证。
