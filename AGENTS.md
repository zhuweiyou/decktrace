# AGENTS.md — decktrace 开发约定

斗地主图像识别记牌器，Rust 实现。面向开发者和 AI Agent：短规则、命令、验收清单。
详细内容放 `docs/` 并在此链接。

## 0. 文档边界

- `README.md`：只面向使用者（安装、使用、权限、常见问题、已知限制）。
- `AGENTS.md`：开发者 / AI Agent 必须遵守的短规则、命令、验收清单。
- `docs/`：详细设计与操作手册：
  - [架构与线程](docs/architecture.md)
  - [识别与调参](docs/recognition.md)
  - [新增游戏](docs/adding-a-game.md)
  - [打包与发布](docs/packaging.md)
  - [Windows 采集路线](docs/windows.md)
  - [安卓端与 ADB 调试](docs/android.md)
  - [开发排障](docs/troubleshooting.md)

规则：README 不写 crate 结构 / 构建测试命令 / 内部性能数字；AGENTS 不写长篇教程；
改架构、识别、打包时同步对应 `docs/`。

## 1. 产品约束

1. **只有“点数/字母下方有花色”的才算普通牌。** 积分、倍数、排名、倒计时、顶部记牌数字不能入账。
2. **同一张牌只扣一次。** 物理身份是 `点数 + 花色`（`AH`、`10S`、`BJ`、`SJ`）；
   手牌 / 底牌展示 / 出牌区可能在不同帧、不同位置重复出现。
3. **底牌认齐才算开局。** 底牌为空、背面、没认全时所有排除都不算数；底牌可见 ≠ 已出。
4. **采集不能被识别拖慢。** 采集回调只入队；识别消费最新帧，队列满丢旧帧。
5. **没有真实画面验证，不算完成。** 必须实际截图 / 启动 GUI（或真机）确认结果。

## 2. 仓库结构

```text
crates/
  tracking/      # 记牌领域：模型、GameProfile/BottomSpec、Recognizer、CardTracker（不依赖 OS/UI/游戏）
  jjddz/         # PC：JJ斗地主（profile + templates + recognition）
  hlddz/         # PC：欢乐斗地主
  hlddz_mobile/  # 手机横屏：欢乐斗地主小程序（已实现识别/素材）
  wzddz/       # 手机：王者斗地主（基础架子：GameProfile + 占位识别器，识别/素材待补）
  capture/       # 采集 trait + WindowInfo/RawFrame + macOS/Windows cfg 实现 + adb 调试后端
  app/           # GUI + Session + registry（唯一桌面发布产物，二进制名 decktrace）
  jni_bridge/    # Android JNI 桥（cdylib -> libjni_bridge.so，create/pushFrame/reset/destroy）
android/         # 安卓 APK 工程（Java：MediaProjection 采集 + 悬浮窗）
xtask/           # cargo xtask package-macos / install-macos / icon / package-android
docs/            # 详细开发文档
```

依赖方向：

```text
tracking
  ↑        ↑         ↑
jjddz   capture   app（应用层）
hlddz
hlddz_mobile / wzddz
```

规则：

- 每个游戏一个独立 crate：`crates/<id>`，**目录名 == `GameProfile.id`**。
- 手机版单独开 crate（`hlddz_mobile` / `wzddz`），不要为兼容手机改 PC 版 profile；模板/区域/比例各自标定。
- 游戏 crate 必须有 `src/profile.rs` 和 `assets/`（识别模板）；打包按此约定扫描。
- GUI 牌面/印章是应用资源：`crates/app/assets/gui/{cards,bottom_badge.png}`；
  `crates/app/assets/AppIcon*` 只给 macOS 打包/Dock，**不进 APK**。
- 所有内部包 `publish = false`；`tracking` 不依赖 OS/UI/具体游戏；平台差异只在 `capture` 用 `cfg`。
- `app` 是组合根，只注入具体游戏与平台实现；识别实现留在各游戏 crate。
- 手机版没有 PC 窗口：不参与 `list_games()` 的窗口自动检测；PC 上只用
  `--capture adb --game <id>` 调试，正式在 APK/JNI 用。
- JNI `NativeBridge.create(assetsDir, gameId)` 按 id 动态选 profile+recognizer；未知 id 返回 0。
- Android 采集帧全部在内存处理，不逐帧落盘；`DEBUG_DUMP_FRAME` 默认 `false`（仅排查时开）。
- Windows 是实验性 GDI（PrintWindow/BitBlt），正式目标是 WGC；见 [docs/windows.md](docs/windows.md)。

## 3. 开发命令

```bash
cargo test --release --workspace                     # 全部测试（必须 release）
cargo run --release -p app                           # 桌面 GUI，自动检测游戏窗口（--game auto）
cargo run --release -p app -- --game hlddz           # 强制指定 PC 游戏
cargo run --release -p app -- --capture adb --game hlddz_mobile --adb-serial <serial>  # 手机 adb 调试
cargo run --release -p app --example tracker_probe -- --adb --serial <serial>          # 无头调牌
cargo build --release -p app                         # 只编译
cargo xtask package-macos                            # 生成 dist/斗地主记牌器.app
cargo xtask install-macos                            # 打包 + 签名 + 装到 /Applications
cargo xtask icon                                     # 由 AppIcon.png 重新生成 icns
cargo xtask package-android                          # 编 JNI .so(arm64-v8a) + gradlew assembleDebug 出 APK
```

- 无窗口时 GUI 显示等待并每 500ms 重试，是预期行为。
- 识别/性能验证必须用 release；debug 下模板匹配和连通域慢一个数量级。

## 4. 识别与底牌约定

- 管线、掩码、模板格式、大小王搜索见 [docs/recognition.md](docs/recognition.md)。
- 点数模板：高度 70，白字黑底，紧贴裁剪；花色模板：40×40，白字黑底。
- 底牌由 `GameProfile.bottom`（`BottomSpec`）决定区域/张数/稳定帧。
- `CardTracker` 在 `armed=false` 时忽略所有排除；底牌仍计入 remaining；双王都识别后跳过王搜索。
- 性能目标：无王 8–10ms/帧，含王 16–25ms/帧。

## 5. GUI / 悬浮窗约定

- 牌面固定 15×30，不缩放；窗口宽度 54 张 + 边距；桌面窗口置顶、不可缩放，标题“斗地主记牌器”。
- 炸弹：某点数剩余 4 张齐全或双王都在，整组反色（不要第三种底色）。
- 底牌：牌面底部居中叠加 9×9 朱红“底”印章，距底 2px，绘制优先级最高。
- Android 悬浮窗：顶部单行牌条，可拖动并持久化坐标；`FLAG_SECURE`（录屏里不拍到自己的牌）。
- 桌面聚焦时按 `R` 重置；退出保存位置；错误提示用 ASCII（egui 默认字体缺中文字形）。

## 6. 测试与验收

```bash
cargo test --release --workspace
```

覆盖：跨帧确认 / 高置信度立即入账 / 帧内去重；底牌未齐不记账、凑齐开局、大小王隔帧；
底牌不扣、打出后扣、无底牌游戏不门控；手动/自动重置正反例；真实截图 golden 回归；
RLE 连通域随机 mask 对拍；54 张牌面素材完整性；`capture` 的 Windows 真实窗口截图集成测试与 adb 解析单测。

手工验收（桌面）：

```bash
cargo run --release -p app
# 或 cargo xtask install-macos
```

检查：窗口标题/置顶/不可缩放；牌条 15×30 相邻；炸弹反色；“底”印不遮主点数；等底牌显示 54 张；
按 `R` 清空；重开回到上次位置。

手工验收（Android）：

```bash
cargo xtask package-android
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

检查：①「授权悬浮窗」/「停止」可用；② 点游戏开始采集；横屏显示悬浮窗牌条；点停止结束。

## 7. 打包

```bash
cargo xtask install-macos   # 日常：打包 + 签名 + 装到 /Applications
cargo xtask package-macos   # 只出 dist/斗地主记牌器.app
cargo xtask package-android # .so + APK（见 docs/android.md）
```

- 打包逻辑在 `xtask/`，不要恢复 shell 脚本。
- macOS：产物名 `斗地主记牌器.app`；bundle id `com.zhuweiyou.decktrace`；可执行名 `decktrace`。
  签名默认找本机 `decktrace-dev` 证书（`--sign` 可覆盖）；ad-hoc 会导致 TCC 重复授权。
  只处理 `crates/<game>/src/profile.rs` 存在的游戏 crate；资源复制到 `Contents/Resources/assets/<game>`（目录名 == id）；
  `Info.plist` 必须写 `Contents/Info.plist`。详见 [docs/packaging.md](docs/packaging.md)。
- Android：`cargo xtask package-android`（默认 release `.so` + `assembleDebug`；`--release` 则 `.so`/APK 都 release）。
  Gradle `assets.srcDirs` 合并 `crates/hlddz_mobile/assets`（templates）与 `crates/app/assets/gui`（cards/、bottom_badge.png）；
  `jniLibs` 由 cargo-ndk 生成（已在 `.gitignore`）。详见 [docs/android.md](docs/android.md)。
- Windows 的 WGC 路线与验证矩阵见 [docs/windows.md](docs/windows.md)。

## 8. 已知问题

排查清单见 [docs/troubleshooting.md](docs/troubleshooting.md)。

## 9. 完成标准

- [ ] 真实截图 / 真实窗口（或真机）验证，保留失败样本
- [ ] 全部 Rust 测试通过（`cargo test --release --workspace`）
- [ ] 识别性能无明显退化
- [ ] macOS：`/Applications/斗地主记牌器.app` 能启动、显示正确、图标正确
- [ ] Android：APK 能安装，悬浮窗/停止/游戏选择正常，采集链路跑通
- [ ] README 只含用户内容；开发内容在 AGENTS/docs
- [ ] 架构 / 识别 / 打包变化已同步 `docs/`
- [ ] 无残留后台进程、无临时文件污染源码目录
