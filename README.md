# 斗地主记牌器（decktrace）

macOS 上的斗地主图像识别记牌器：持续截取目标窗口，识别画面中出现的牌，按“点数 + 花色”
去重后，用置顶牌条展示剩余牌。

采集、识别、游戏规则、UI 都是可替换的。当前默认实现面向微信小程序《JJ斗地主》。

- **纯 Rust**：ScreenCaptureKit 采集，无 Swift helper、无 Python 运行时
- **置顶 GUI**：固定 15×30 像素牌条，炸弹反色，底牌叠加朱红“底”印章
- **底牌开局门控**：识别到三张底牌后才开始记牌，避免发牌/翻牌动画误扣
- **自动重置**：新手牌跳变或整体换牌时清空上一局

## 快速开始

要求：macOS 12.3+、Xcode Command Line Tools、Rust 1.88+。

```bash
# 打包并启动 dist/decktrace.app
./start-gui.sh

# 只打包
./scripts/bundle-macos.sh

# 开发时直接运行（会继承终端已有的屏幕录制权限）
cargo run --release -p decktrace
```

`decktrace.app` 的内部可执行文件名为 `decktrace`。首次通过 `.app` 启动时，macOS 会要求
屏幕录制权限：系统设置 → 隐私与安全性 → 屏幕与系统音频录制。

## 架构

```text
crates/
  decktrace-core/             # 模型、GameProfile/BottomSpec、Recognizer/CaptureStream 抽象、CardTracker
  decktrace-game-jj/          # JJ 实现：窗口、牌堆、模板、牌面素材、识别引擎
  decktrace-capture/          # 平台实现：macOS cfg / Windows cfg（Windows 占位）
  decktrace/                  # GUI + Session + 游戏注册表（唯一发布产物）
scripts/
  bundle-macos.sh             # 生成 dist/decktrace.app
```

数据流：

```text
WindowService → CaptureStream → Session → CardRecognizer → CardTracker → GUI
```

- `GameProfile` / `BottomSpec` / `Recognizer` / `CaptureStream` 抽象在 `decktrace-core`。
- 平台实现按 `cfg(target_os)` 收在 `decktrace-capture`，不会为另一个平台编译依赖。
- 模板识别实现属于 JJ：`decktrace-game-jj/src/recognition.rs`。
- `Session` 只依赖 core 的 `Recognizer` 和 `CaptureStream`；`decktrace` 负责把 JJ 实现注入进去。
- 平台只负责窗口和帧流；识别只产出 `Detection`；UI 只消费 `TrackerSnapshot + GameProfile`。

## 识别规则

1. 用 red/dark 颜色掩码找花色候选；
2. 花色正上方必须存在点数；
3. 点数、花色都用二值模板做 IoU 匹配；
4. 周围必须是浅色卡面；
5. 大小王没有花色，用 JOK 模板在高度带内粗搜索 + 局部 CCOEFF 精验。

牌的身份是 `点数 + 花色`（如 `AH`、`10S`、`BJ`、`SJ`），同一张牌只扣一次。

### 底牌门控

- 底牌识别由游戏 profile 提供：`GameProfile.bottom`（`BottomSpec`）。
- JJ 使用固定区域 `(0.33, 0.08, 0.47, 0.20)`，需要连续稳定识别到 3 张底牌才 `armed=True`。
- 底牌出现前，手牌、翻牌动画、中间大牌检测一律不记账。
- 底牌仍计入剩余；只有真正打出（出现在非底牌区）后才扣除。
- GUI 中底牌在牌面底部叠加 9×9 朱红“底”印章（距底 2px），不改变牌面本身。

### 炸弹提示

- 某点数 4 张齐全，或大小王都在时，该组牌反色显示。
- 底牌印章叠加在反色/正色之上，不受影响。

## 性能

release、1800×1100：

- 不做大小王搜索：约 **8–10ms/帧**
- 做大小王搜索：约 **16–25ms/帧**
- 大小王都已有下落后，运行时自动跳过王搜索
- 采集目标 15fps

分段计时：

```bash
DECKTRACE_TIMING=1 cargo run --release -p decktrace
```

实现要点：

- red/dark/combined 一次并行扫描
- 行程编码（RLE）+ 两趟并查集的连通域
- 缩小图 CCOEFF 选候选，再局部全分辨率精验
- 普通牌识别与大小王识别并行

## 换游戏

新建一个游戏 crate：

1. `crates/decktrace-game-xxx`
2. 实现 `GameProfile`：窗口特征、牌堆、模板目录、牌面素材、底牌策略、重置规则
3. 在 `crates/decktrace/src/registry.rs` 加一个分支，并注入该游戏的 `Recognizer` 实现

没有底牌概念的游戏把 `bottom` 设为 `None`，tracker 会直接开始记牌。
底牌识别方式不是通用的：可以实现自己的 `BottomSpec`。

## 打包

`scripts/bundle-macos.sh` 会：

- 构建 `decktrace` release 二进制
- 生成 `dist/decktrace.app/Contents/MacOS/decktrace`
- 拷贝 `crates/decktrace-game-jj/assets` 到 `Contents/Resources/assets/jj`
- 写入 `Info.plist`（含 `NSScreenCaptureUsageDescription`）
- 做本机 ad-hoc 签名

正式分发需要 Developer ID 签名和公证。

## 测试

```bash
cargo test --release --workspace
```

- 牌堆、底牌门控、自动重置：`crates/decktrace-game-jj/tests/tracker.rs`
- 真实截图识别回归：`crates/decktrace-game-jj/tests/golden.rs`
- 连通域随机 mask 对拍：`engine::tests`

截图回归依赖仓库根目录的 `shots/`；样本缺失时自动跳过。把真实牌局截图放进 `shots/`
即可本地启用。

## 已知限制

- 仅实现 macOS 采集；Windows 保留 trait，尚未实现
- 程序从一局中间启动时，无法补回启动前已经出过的牌
- 模板与当前游戏字体/皮肤绑定，换皮肤需要更新模板和素材
- `.app` 首次运行需要手动授予屏幕录制权限
