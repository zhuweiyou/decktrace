# AGENTS.md — decktrace 开发约定

本项目是斗地主图像识别记牌器，Rust 实现。后续 Agent 修改时优先遵循这里的约定；
发现更好的方案，先验证，再更新本文件。

## 0. 文档边界

- `README.md`：只面向使用者。安装、使用、权限、常见问题、已知限制。
- `AGENTS.md`：Agent/开发者必须遵守的短规则、命令、验收清单。
- `docs/`：详细设计和操作手册，按需阅读：
  - [架构与线程](docs/architecture.md)
  - [识别与调参](docs/recognition.md)
  - [新增游戏](docs/adding-a-game.md)
  - [打包与发布](docs/packaging.md)
  - [Windows 采集路线](docs/windows.md)
  - [安卓端与 ADB 调试](docs/android.md)
  - [开发排障](docs/troubleshooting.md)

规则：

- README 不写 crate 结构、库列表、构建/测试命令、内部性能数字。
- AGENTS 不写长篇教程，详细内容放 `docs/` 并在这里链接。
- 修改架构、识别、打包流程时，同步对应 `docs/`。

## 1. 产品约束

1. **只有“点数/字母下方有花色”的才算普通牌。**
   - 积分、倍数、排名、倒计时、顶部记牌器数字不能入账。
2. **同一张牌只扣一次。**
   - 物理身份是 `点数 + 花色`（`AH`、`10S`、`BJ`、`SJ`）。
   - 手牌、底牌展示、出牌区可能在不同帧/位置重复出现。
3. **底牌识别到齐，才算游戏开始。**
   - 底牌区为空、背面或没认全时，所有排除都不算数。
   - 底牌可见 ≠ 已出：底牌仍计入剩余，只有真正打出后才扣除。
4. **采集不能被识别拖慢。**
   - 采集回调只负责入队；识别消费最新帧，队列满丢旧帧。
5. **没有真实画面验证，不算完成。**
   - 必须实际截图/启动 GUI，确认识别和显示结果。

## 2. 仓库结构

```text
crates/
  tracking/    # 记牌领域：模型、GameProfile/BottomSpec、Recognizer、CardTracker
  jjddz/       # JJ斗地主适配：profile、templates、recognition
  hlddz/       # 欢乐斗地主适配：profile、templates、recognition
  hlddz_mobile/ # 欢乐斗地主手机横屏小程序适配（独立于 hlddz，模板/区域单独标定）
  wzryddz/     # 王者荣耀斗地主（基础架子：GameProfile + 占位识别器，识别/素材待补）
  capture/     # 采集 trait + WindowInfo/RawFrame + macOS/Windows cfg 实现 + adb 调试后端
  app/   # GUI + Session + registry（唯一发布产物）
  jni_bridge/ # Android JNI 桥（cdylib -> libjni_bridge.so）
android/       # 安卓 APK 工程（Java：MediaProjection 采集 + 悬浮窗）
xtask/         # cargo xtask package-macos / install-macos / icon
docs/          # 详细开发文档
```

依赖方向：

```text
tracking
  ↑        ↑         ↑
jjddz   capture   app（应用层）
hlddz
hlddz_mobile
```

规则：

- 每个游戏一个独立 crate：`crates/<id>`，目录名必须等于 `GameProfile.id`。
- 手机版单独开 crate（如 `hlddz_mobile`），**不要为兼容手机去改 PC 版 profile**；模板/区域/比例按平台各自标定。
- 游戏 crate 必须有 `src/profile.rs` 和 `assets/`（识别模板）；打包靠这个约定扫描。
- GUI 牌面 `cards/` 是应用资源，放在 `crates/app/assets/cards`，所有游戏共用。
- 所有内部包 `publish = false`。
- `tracking` 不依赖 OS、UI、具体游戏。
- 识别实现属于各游戏 crate（如 `jjddz/src/recognition.rs`），不要恢复独立识别包。
- `capture` 自己持有采集 trait 和平台实现，平台差异只用 `cfg`。
- `app` 是组合根，只负责注入具体游戏和平台实现。
- 应用默认 `--game auto`；`--game <id>` 仅用于调试覆盖。
- 详细架构见 [docs/architecture.md](docs/architecture.md)。
- Windows 当前是实验性 GDI，正式目标是 WGC；实现步骤和验证矩阵见 [docs/windows.md](docs/windows.md)。

## 3. 开发命令

```bash
cargo test --release --workspace
cargo run --release -p app                  # 自动检测游戏窗口（默认 auto）
cargo run --release -p app -- --game hlddz  # 调试时强制指定
cargo build --release -p app                # 只编译
cargo xtask package-macos                         # 生成 dist/斗地主记牌器.app
cargo xtask install-macos                         # 打包 + 签名 + 安装到 /Applications
cargo xtask icon                                  # 由 AppIcon.png 重新生成 icns
cargo xtask package-android                       # 编 JNI .so(arm64-v8a) + gradlew assembleDebug 出 APK
open dist/斗地主记牌器.app                         # 运行打包产物
```

无窗口时程序显示等待并每 500ms 重试，这是预期行为。

识别/性能验证必须用 release。debug 下模板匹配和连通域会慢一个数量级。

## 4. 识别与底牌约定

- 管线、掩码、模板格式、大小王搜索见 [docs/recognition.md](docs/recognition.md)。
- 点数模板：高度 70，白字黑底，紧贴裁剪。
- 花色模板：40×40，白字黑底。
- 底牌由 `GameProfile.bottom` 提供，`BottomSpec` 决定区域/张数/稳定帧。
- `CardTracker` 在 `armed=false` 时忽略所有排除；底牌仍计入 remaining。
- 没有底牌概念的游戏设 `bottom=None`。
- 双王都识别后跳过后续王搜索。
- 性能目标：无王 8–10ms/帧，含王 16–25ms/帧。

## 5. GUI 约定

- 牌面固定 15×30，不缩放；窗口宽度固定 54 张 + 边距。
- 炸弹：某点数剩余 4 张齐全，或双王都在，整组反色。不要改成第三种底色。
- 底牌：不改变牌面底色，在牌面底部居中叠加 9×9 朱红“底”印章，距底 2px。
- 底牌印章绘制在反色/正色之后，优先级最高。
- 牌面素材：`crates/app/assets/cards`，应用层共用。
- 印章素材：`crates/app/assets/bottom_badge.png`，通过 `include_bytes!` 嵌入。
- 窗口置顶、不可缩放；标题为“斗地主记牌器”。
- 窗口聚焦时按 `R` 重置本局；退出时保存窗口位置。
- 错误提示用 ASCII，避免 egui 默认字体缺中文字形。

## 6. 测试与验收

```bash
cargo test --release --workspace
```

覆盖：

- 跨帧确认、高置信度立即入账、帧内去重
- 底牌未齐不记账、凑齐后开局、大小王隔帧
- 底牌不扣、打出后扣、无底牌游戏不门控
- 手动/自动重置正反例
- 真实截图 golden 回归
- RLE 连通域随机 mask 对拍
- 54 张牌面素材完整性

手工验收：

```bash
cargo run --release -p app
# 或
cargo xtask install-macos
```

检查：

- 窗口标题、置顶、不可缩放
- 牌条 15×30 相邻排列
- 炸弹组反色
- 底牌有朱红“底”印，不遮住主要点数
- 等待底牌时显示全部 54 张
- 按 `R` 能清空本局
- 关闭再启动，窗口回到上次位置

## 7. 打包

```bash
cargo xtask install-macos   # 日常用这个：打包 + 签名 + 装到 /Applications
cargo xtask package-macos   # 只出 dist/斗地主记牌器.app
```

- 打包逻辑在 `xtask/`，不要恢复 shell 脚本。
- 产物名是 `斗地主记牌器.app`；bundle id `com.zhuweiyou.decktrace`。
- 签名默认找本机 `decktrace-dev` 自签名证书（`--sign` 可覆盖）；ad-hoc 会导致 TCC 重复授权。
- 只处理 `crates/<game>/src/profile.rs` 存在的游戏 crate。
- 资源复制到 `Contents/Resources/assets/<game>`；目录名必须等于 `GameProfile.id`。
- `Info.plist` 必须写到 `Contents/Info.plist`（写进 Resources 会导致 codesign 失败）。
- macOS Swift runtime、Info.plist、签名细节见 [docs/packaging.md](docs/packaging.md)。
- Windows 的 WGC 路线、验证矩阵和后续 `package-windows` 计划见 [docs/windows.md](docs/windows.md)。

## 8. 已知问题

排查清单见 [docs/troubleshooting.md](docs/troubleshooting.md)。

## 9. 完成标准

- [ ] 真实截图/真实窗口验证，保留失败样本
- [ ] 全部 Rust 测试通过
- [ ] 识别性能无明显退化
- [ ] `/Applications/斗地主记牌器.app` 能启动并正常显示，图标正确
- [ ] README 只包含用户内容；开发内容在 AGENTS/docs
- [ ] 架构/识别/打包变化已同步 `docs/`
- [ ] 无残留后台进程、无临时文件污染源码目录
