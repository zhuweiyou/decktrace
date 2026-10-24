# AGENTS.md — decktrace 开发约定

本项目是 macOS 上的斗地主图像识别记牌器，Rust 实现。后续 Agent 修改时优先遵循这里的约定；
发现更好的方案，先验证，再更新本文件。

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
   - ScreenCaptureKit 回调只负责入队；识别消费最新帧，队列满丢旧帧。
5. **没有真实画面验证，不算完成。**
   - 必须实际截图/启动 GUI，确认识别和显示结果。

## 2. 仓库结构

```text
crates/
  decktrace-tracking/         # 牌模型、GameProfile/BottomSpec、Recognizer、CardTracker
  decktrace-jj/               # JJ 实现：profile、templates、cards、recognition
  decktrace-capture/          # 采集 trait + WindowInfo/RawFrame + macOS/Windows 实现（cfg）
  decktrace/                  # GUI + Session + registry（二进制 decktrace）
scripts/bundle-macos.sh       # 打包 decktrace.app
start-gui.sh                  # 打包并启动
```

规则：

- 每个游戏一个独立 crate：`decktrace-<game>`（例如 `decktrace-jj`）。新增游戏在 `decktrace` 的 `registry.rs` 加分支。
- 通用层只依赖 `decktrace-tracking` 的 `GameProfile` / `BottomSpec` / `Recognizer`，以及 `decktrace-capture` 的采集 trait。
- capture 的抽象和实现内聚在 `decktrace-capture`，平台差异只用 `cfg` 和 target-specific dependencies。
- 模板识别是 JJ 专属实现，放在 `decktrace-jj/src/recognition.rs`，不要恢复独立识别 crate。
- 发布产物只有 GUI，不提供 TUI 二进制。

## 3. 开发命令

```bash
cargo test --release --workspace
cargo run --release -p decktrace
./start-gui.sh                 # 打包并打开 dist/decktrace.app
./scripts/bundle-macos.sh      # 只打包
```

识别/性能验证必须用 release。debug 下模板匹配和连通域会慢一个数量级。

## 4. 识别管线

```text
red/dark 掩码 → 连通域 → 花色候选过滤 → 花色上方定位点数
→ 点数/花色模板 IoU → 卡面亮度校验 → 同一帧 NMS → 大小王搜索 → 底牌标记
```

- 掩码：red 与 dark 一次并行扫描，同时产出 combined。
- 连通域：行程编码（RLE）+ 两趟并查集。修改后必须跑 `engine::tests` 的随机 mask 对拍。
- 点数/花色：点数框归一化到 70px 高，再与模板做 IoU；上采样用 nearest。
- 大小王：高度带 + 多尺度，先 1/4 缩小图 CCOEFF 选候选，再全分辨率局部 CCOEFF 精验。
- 性能：无王搜索约 8–10ms/帧，含王搜索约 16–25ms/帧。双王都有下落后跳过王搜索。

调试：

```bash
DECKTRACE_TIMING=1 cargo run --release -p decktrace
```

真实截图回归在 `crates/decktrace-jj/tests/golden.rs`，依赖根目录 `shots/`；
样本缺失时 skip。修改识别逻辑后必须跑完整 `cargo test --release --workspace`。

## 5. 底牌门控

- 底牌识别是游戏能力：`GameProfile.bottom: Option<Arc<dyn BottomSpec>>`。
- JJ 使用 `BottomConfig(region=(0.33,0.08,0.47,0.20), count=3, stable_frames=2)`。
- `CardTracker` 在 `armed=false` 时忽略所有排除，只收集底牌区检测。
- 连续 stable_frames 帧出现同样的 count 张底牌后开局；大小王隔帧检测允许中间少一张。
- 开局那一帧跳过底牌动画残影。
- 开局后底牌仍计入 remaining；打出/进入手牌后从 `bottom_ids` 移除。
- 没有底牌概念的游戏设 `bottom=None`，tracker 一开始就是 armed。

## 6. GUI 约定

- 牌面固定 15×30，不缩放；窗口宽度固定 54 张 + 边距。
- 炸弹：某点数剩余 4 张齐全，或双王都在，整组反色。不要改成第三种底色。
- 底牌：不改变牌面底色，在牌面底部居中叠加 9×9 朱红“底”印章，距底 2px。
- 底牌印章绘制在反色/正色之后，优先级最高。
- 印章素材：`crates/decktrace/assets/bottom_badge.png`，通过 `include_bytes!` 嵌入二进制。
- 窗口置顶、不可缩放；标题为“斗地主记牌器”。

## 7. macOS 采集与打包

- 窗口枚举与帧流都走 ScreenCaptureKit；`CVPixelBuffer` 在进程内转 RGB。
- `screencapturekit` 的 Swift 桥接需要 Swift 运行时：
  - `.cargo/config.toml` 给最终二进制加 `-rpath /usr/lib/swift`
  - `crates/decktrace-capture/build.rs` 把 CLT 的 Swift 静态兼容库目录加入 link search
  这两处不要删，否则 dyld 会找不到 `libswift_Concurrency.dylib`。
- 资源路径解析：`DECKTRACE_ASSETS` → `.app/Contents/Resources/assets/jj` → 开发目录。
- 打包脚本会拷贝 `crates/decktrace-jj/assets` 到 `Contents/Resources/assets/jj`。
- `Info.plist` 必须包含 `NSScreenCaptureUsageDescription`。
- 首次运行需要屏幕录制权限；直接终端运行会继承终端权限，`.app` 需要单独授权。
- 正式分发需要 Developer ID 签名和公证；当前脚本只是 ad-hoc 签名。

## 8. 测试与验收

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

手工验收：

```bash
./start-gui.sh
```

检查：

- 窗口标题“斗地主记牌器”，置顶、不可缩放
- 牌条 15×30 相邻排列
- 炸弹组反色
- 底牌底部有朱红“底”印，不遮住主要点数
- 等待底牌时显示全部 54 张，不提前扣牌
- 窗口聚焦时按 `R` 能清空本局

## 9. 已知坑

| 现象 | 根因 | 修复 |
| --- | --- | --- |
| dyld 找不到 libswift_Concurrency | 最终二进制没有 Swift runtime rpath | 保留 `.cargo/config.toml` 的 rpath |
| `.app` 里模板/牌面丢失 | 资源路径是编译期绝对路径 | `asset_root()` 解析 `Contents/Resources` |
| 翻牌动画被误扣 | 底牌没认全就开始记牌 | `armed=false` 忽略所有排除 |
| 开局前手牌被计入 | 同上 | 三张底牌稳定后才开局 |
| 底牌被提前扣除 | 把底牌区当普通检测 | 底牌单独 `bottom_ids`，不扣剩余 |
| 大小王漏检 | 底牌栏压缩字形/尺度不同 | 紧凑模板 + 高度带 + 多尺度 |
| 识别慢 | 全图多尺度模板搜索 | 高度带 + 缩小图选候选 + 局部精验 |
| 8 连通统计错误 | 通用逐像素 union-find 慢/易错 | RLE 两趟并查集 + 随机 mask 对拍 |
| `.app` 首次启动无画面 | 没授权屏幕录制 | 系统设置授予权限后重启 |
| GUI 资源找不到 | 只拷贝了二进制，没拷贝 assets | 用 `scripts/bundle-macos.sh` |

## 10. 完成标准

- [ ] 真实截图/真实窗口验证，保留失败样本
- [ ] 全部 Rust 测试通过
- [ ] 识别性能无明显退化
- [ ] `dist/decktrace.app` 能启动并正常显示
- [ ] README/AGENTS 同步
- [ ] 无残留后台进程、无临时文件污染源码目录
