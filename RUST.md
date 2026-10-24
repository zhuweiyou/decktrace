# decktrace Rust 重写

本分支（`rust-rewrite`）把原来的 Python + Swift helper 重写成 Rust 多 crate workspace。
Python 版本暂时保留，作为识别结果的对照 oracle；Rust 侧跑通并完成迁移后再删除。

## 当前状态

已在 macOS 本机跑通：

- 窗口枚举 + ScreenCaptureKit 实时采集（纯 Rust，无 Swift helper）
- 模板识别（点数/花色/大小王/底牌区），6 张真实截图与 Python 版结果一致
- 三张底牌开局门控、底牌不提前扣除、自动重置
- 独立 GUI 应用（置顶牌条，普通/底牌蓝/炸弹反色）

尚未做：

- Windows 采集后端（只保留 trait 抽象）
- GUI 的拖动位置持久化、错误提示细节、`.app` 打包/签名

## Workspace

```text
crates/
  decktrace-core/             # 模型、牌堆规则、CardTracker、底牌门控
  decktrace-game-spec/        # 游戏接入契约：GameProfile / BottomSpec
  decktrace-game-jj/          # JJ 斗地主实现：配置、模板、牌面素材
  decktrace-games/            # 游戏注册表（新增游戏在这里注册一行）
  decktrace-recognition/      # 通用模板识别引擎
  decktrace-capture/          # WindowService / CaptureStream / Platform trait
  decktrace-capture-macos/    # ScreenCaptureKit 实现
  decktrace-session/          # 无 UI 运行时：采集→识别→状态机、自动重置
  decktrace/                  # GUI 应用：egui 置顶牌条（二进制 decktrace，打包 decktrace.app）
```

### Crate 职责

- `decktrace-session`：headless，不依赖 clap/crossterm/egui，GUI 和测试共用。
- `decktrace`：唯一的发布应用。自己驱动 egui 事件循环，打包为 `decktrace.app`。
- `decktrace-game-spec`：只定义游戏契约；`decktrace-game-jj` 才是 JJ 实现，二者不要混淆。
- `decktrace-games`：注册表，负责把各游戏 crate 组合起来。
- TUI 不再作为发布产物，也不再有独立二进制；测试直接 `cargo test`。

新增游戏时：

1. 新建 `crates/decktrace-game-xxx`
2. 实现 `GameProfile`（窗口、牌堆、模板、底牌策略、重置规则）
3. 在 `decktrace-games` 的注册表里加一行

## 命令

```bash
# 打包并启动 decktrace.app
./start-gui.sh

# 开发时直接跑
cargo run --release -p decktrace

# 只打包不启动
./scripts/bundle-macos.sh

# 测试（识别回归必须在 release 下跑，debug 会慢很多）
cargo test --release --workspace
```

## 识别对拍

`crates/decktrace-recognition/tests/golden.rs` 使用 `shots/` 里的真实截图，和 Python 版导出的
期望集合逐张比较；样本不存在时跳过。当前覆盖：

- `round_now.png`：26 张，底牌 `10S / AH / 8H`
- `round_live.png`：底牌背面，只识别手牌/出牌，不开局
- `round_live2.png`：21 张，底牌 `7C / 9D / 9H`
- `bj_bottom_real.png`：20 张，底牌大小王与手牌大小王共存
- `joker_miss.png`：22 张，压缩小王在底牌区
- `shots/` 全量：可运行 `DECKTRACE_TIMING=1` 看 masks/cards/jokers 分段耗时

## 性能

实测（release，1800×1100）：

- 不做大小王搜索：约 **8–10ms/帧**
- 做大小王搜索：约 **16–25ms/帧**
- 大小王都已有下落（底牌/已见）后，运行时自动完全跳过王搜索
- 采集目标 15fps，实时运行轻松跑满

之前 Rust 版 50ms 的原因和优化过程：

1. 红/黑掩码、普通牌、大小王搜索全是串行
   → 改成红/黑并行、普通牌与大小王并行。
2. 掩码逐像素 `put_pixel` 且额外再算一遍 combined
   → 一次并行扫描直接产出 red/dark/combined，4ms → 1ms。
3. `imageproc` 的通用 union-find 连通域 + 全图像素统计
   → 换成行程编码（RLE）+ 两趟并查集，只处理前景行程；并有随机 mask 对拍测试。
4. 大小王粗搜索用 `CCORR_NORMED`，误候选多，每个都要全分辨率精验
   → 缩小图直接算 CCOEFF_NORMED 选候选，再少量精验。
5. 双王都有下落后仍然每帧搜王
   → `jokers_known()` 后完全跳过。
6. 对比口径：Python 的 15–25ms 是“王确认后”的路径，含王搜索时 Python 实测约 95–113ms；
   不能拿它直接对比 Rust 的含王路径。

后续如果还要压，优先看大小王精验的积分图/SIMD 和分类模板的批量归一化。

## macOS 采集

- 窗口枚举和帧流都走 ScreenCaptureKit，直接拿到 `CVPixelBuffer`，在进程内转 RGB，
  不再编码 JPEG、也不再启动 Swift helper。
- `screencapturekit` 的 Swift 桥接需要 Swift 运行时。`.cargo/config.toml` 给最终二进制加了
  `-rpath /usr/lib/swift`，`crates/decktrace-capture-macos/build.rs` 负责把 CLT 的静态
  兼容库目录加入 link search。换机器/换 Xcode 后如果 dyld 报找不到 Swift 库，先检查这两处。
- 首次运行会触发屏幕录制权限；和 Python 版一样，运行终端需要授权。
- 用 release 跑实时识别。debug 下模板匹配/连通域会慢一个数量级。

## 关键设计

- 底牌识别不是通用引擎常量：`GameProfile.bottom` 注入 `BottomSpec`；
  JJ 用 `BottomConfig(region=(0.33,0.08,0.47,0.20), count=3, stable_frames=2)`。
- `CardTracker` 在底牌确认前忽略所有检测，避免发牌/翻牌动画误扣。
- 大小王用“粗搜索 + 局部精确 CCOEFF_NORMED”：先 1/4 缩放做 CCORR_NORMED 找候选，
  再在全分辨率小窗口里算 CCOEFF_NORMED，兼顾速度和精度。
