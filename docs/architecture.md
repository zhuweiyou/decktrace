# 架构与线程模型

## crate 结构

```text
crates/
  tracking/    # 记牌领域：模型、GameProfile/BottomSpec、Recognizer 契约、CardTracker
  jjddz/       # JJ斗地主适配：profile、识别模板、识别实现
  hlddz/       # 欢乐斗地主适配：profile、识别模板、识别实现
  capture/     # 采集契约 + WindowInfo/RawFrame + macOS/Windows cfg 实现
              # Windows 为实验 GDI 实现
  decktrace/   # GUI + Session + registry，唯一发布产物
xtask/         # 开发任务：package-macos / install-macos / icon
```

依赖方向：

```text
tracking
  ↑        ↑         ↑
jjddz   capture   decktrace（应用层）
hlddz
```

规则：

- `tracking` 不依赖 OS、UI、具体游戏。
- 每个游戏 crate 只依赖 `tracking`，实现自己的 `Recognizer`。
- GUI 牌面 `cards/` 属于应用层 `crates/decktrace/assets/cards`，不属于游戏。
- `capture` 自己持有采集 trait 和平台实现，平台差异只用 `cfg` 和 target-specific dependencies。
- `decktrace` 是组合根，注入具体游戏识别器和平台实现。
- 所有内部包 `publish = false`。

## 数据流

```text
ScreenCaptureKit
  → CaptureStream
  → RawFrame (BGRA → RGB)
  → Session worker
  → Recognizer
  → CardTracker
  → snapshot channel
  → egui GUI
```

## 线程

- **主线程**：egui 渲染、贴图缓存、窗口位置保存。
- **capture worker**：窗口检测、采集、识别、记牌。
- **ScreenCaptureKit 回调**：只负责把帧放入有界队列。
- 识别消费最新帧；队列满时丢旧帧，不让采集被识别拖慢。
- GUI snapshot 通道 `bounded(1)`，新快照覆盖旧快照。
- 默认采集 10 FPS；画面几乎静止时跳过识别，GUI 每 80ms 重绘。

## 自动检测游戏

- `--game auto` 是默认值。
- worker 调用 `list_games()`，对每个 `GameProfile.window` 做窗口匹配。
- 只匹配到一个游戏：直接使用。
- 多个匹配：按注册顺序使用第一个，并在 stderr 提示。
- 无匹配：GUI 显示等待，worker 每 500ms 重试。
- `--game <id>` 只用于调试或强制覆盖。
- `list_games()` 和 `create_recognizer()` 是新游戏的注册入口。

## 技术栈

| 库 | 用途 |
| --- | --- |
| `eframe` / `egui` 0.37 | GUI、置顶窗口、贴图 |
| `screencapturekit` 11.1 | macOS ScreenCaptureKit |
| `image` 0.25 | 图片解码、RGB 帧、牌面素材 |
| `imageproc` 0.27 | 图像辅助操作 |
| `rayon` 1.10 | 掩码、连通域、大小王搜索并行 |
| `crossbeam-channel` 0.5 | 采集队列、GUI snapshot |
| `clap` 4.6 | CLI 参数 |
| `anyhow` / `thiserror` | 应用/模块错误 |

## 扩展点

- 新游戏：见 [adding-a-game.md](adding-a-game.md)。
- 新平台：在 `capture` 内增加 `cfg` 模块，不要让 `Session` 或游戏层出现平台判断。
- Windows 当前为实验 GDI；WGC 正式方案和验证矩阵见 [windows.md](windows.md)。
- 识别实现：跟随游戏 crate，不恢复独立通用识别包。
