# 跨平台斗地主图像识别记牌器

通过窗口采集识别牌面，自动扣除自己的手牌、底牌和所有人出过的牌，在 TUI 或置顶 GUI 中展示剩余牌。

当前 **macOS 完整可用**；Windows 已完成窗口枚举与架构预留，截图/采集后端待实现。

## 架构

目标是让 **平台、游戏、识别来源** 三件事互不影响：

```text
Platform（平台）
  ├─ macos: ScreenCaptureKit 常驻采集 / screencapture 回退
  └─ windows: 窗口枚举已实现，CaptureStream TODO

GameProfile（游戏）
  ├─ 窗口特征（标题/应用/尺寸）
  ├─ 牌堆定义（每个点数几张、显示顺序、大小王标签）
  ├─ 牌面模板目录与识别阈值
  └─ 自动重置规则

DetectionSource（识别来源）
  ├─ image: 平台采集流 + 模板识别
  └─ memory: 预留（内存扫描/读牌），只需产出 DetectionBatch

Core（业务，完全复用）
  ├─ CardTracker：按“点数+花色”去重、计数
  ├─ Runtime：消费 DetectionBatch、自动重置
  └─ UI 抽象
      ├─ TUI：Rich 表格，显示 fps/队列等调试信息
      └─ GUI：Qt 置顶窗口，用固定 15×30 牌面左侧条展示剩余牌
```

### UI 抽象

`core/ui/` 只依赖 `TrackerSnapshot + GameProfile`：

- `TuiRenderer`：Rich 表格，保留帧率、队列、识别耗时等调试信息。
- `QtCardRenderer`：纯展示，一行牌面图片，不显示 fps；窗口默认置顶。
- `CardFaceFactory`：读取 `games/<id>/assets/cards/<card_id>.png`，按原始 15×30
  像素尺寸展示（最近邻，不放大）。
- 某点数剩余 4 张齐全、或双王都在时，对应牌条反色高亮；可用
  `--no-highlight-bombs` 关闭。

当前 GUI 使用项目所有者提供的《hlddz》牌面左侧条素材。

### 目录结构

```text
jjcounter/
├── cli.py                     # list / shot / watch / run / games
├── core/
│   ├── models.py              # WindowInfo / WindowSelector / Detection / DetectionBatch
│   ├── source.py              # DetectionSource 抽象
│   ├── deck.py                # 去重计数
│   ├── table.py               # Rich 记牌表
│   ├── runtime.py             # 通用运行时（不依赖平台/来源）
│   └── ui/
│       ├── base.py            # Renderer 抽象
│       ├── tui.py             # Rich TUI
│       ├── qt.py              # Qt 置顶牌面窗口
│       ├── cards.py           # 牌面图片加载
│       └── highlight.py       # 炸弹高亮判断
├── platforms/
│   ├── base.py                # WindowService / CaptureStream / Platform
│   ├── factory.py             # 按 sys.platform 选择后端
│   ├── macos/
│   │   ├── window.py          # native/winlist.swift
│   │   ├── sck.py             # ScreenCaptureKit 二进制帧客户端
│   │   ├── capture.py         # SCK / screencapture 两条采集流
│   │   ├── legacy.py          # 单帧 screencapture
│   │   └── native/            # winlist.swift / sckcap.swift
│   └── windows/
│       ├── window.py          # ctypes EnumWindows（实验性）
│       ├── capture.py         # TODO：CaptureStream 实现
│       └── platform.py
├── games/
│   ├── base.py                # GameProfile / RecognitionConfig / ResetConfig
│   ├── registry.py            # 游戏注册表
│   └── jj/
│       ├── profile.py         # JJ 斗地主配置
│       ├── templates/         # 识别用点数/花色/大小王模板
│       ├── assets/cards/      # GUI 用 15×30 牌面左侧条
│       └── build_templates.py
├── recognition/
│   └── engine.py              # 通用模板识别引擎（读取 GameProfile）
└── sources/
    ├── image.py               # 图片识别来源（采集流 + recognizer）
    ├── memory.py              # 内存来源 TODO
    └── factory.py
```

## 快速开始

```bash
cd /Users/yiyi/work/jjddz
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt \
  -i https://pypi.tuna.tsinghua.edu.cn/simple

./start.sh
```

运行后：

- `r`：重置本局
- `q` / `Ctrl+C`：退出

常用命令：

```bash
./start.sh games                         # 查看已注册游戏
./start.sh list                          # 查看窗口
./start.sh list --game jj                # 按游戏窗口特征标记
./start.sh shot --out shots/now.png      # 单张截图
./start.sh run --debug                   # TUI 实时记牌 + 标注图
./start.sh run --once --debug            # 单帧自检
./start.sh watch --interval 0.5          # 只采集、不识别
```

### GUI（置顶牌面窗口）

GUI 需要额外安装 Qt：

```bash
.venv/bin/python -m pip install -r requirements-gui.txt   -i https://pypi.tuna.tsinghua.edu.cn/simple

./start.sh run --ui gui
```

- 固定窗口宽度，正好放下 54 张 15×30 牌条
- 一行排列，牌条相邻、无间距
- 默认窗口置顶：`--no-topmost` 可关闭
- 不显示 fps/队列，只展示剩余牌
- 炸弹/王炸反色高亮：`--no-highlight-bombs` 可关闭

TUI 仍是默认，方便调试：

```bash
./start.sh run            # TUI
./start.sh run --ui gui   # GUI
```

### 平台/采集后端

```bash
# 默认：自动选择 SCK，失败回退 screencapture
./start.sh run

# 强制 ScreenCaptureKit
./start.sh run --capture-backend sck

# 强制传统 screencapture
./start.sh run --capture-backend legacy

# 指定平台（Windows 采集目前会明确报 TODO）
./start.sh list --platform windows
```

macOS 采集配置：

```bash
./start.sh run --capture-fps 15 --capture-quality 0.88
```

## 当前性能

- 常驻 ScreenCaptureKit：配置 15fps，实测约 **13.6–14fps**
- 双王确认后识别单帧约 **15–25ms**
- 未确认双王时大小王隔帧检测，约 **95ms/帧**
- 队列只保留 3 帧，延迟约 **0.1–0.3s**
- Rich 表格每 0.12s 刷新一次
- GUI 更新只重绘牌条，不显示 fps

采集与识别线程分离；识别慢不会拖慢采集，队列满会丢最旧帧。

## 识别规则

1. 用颜色掩码找红色/黑色花色候选；
2. 花色正上方必须存在点数；
3. 点数和花色都通过模板匹配；
4. 周围必须是浅色卡面。

所以积分、倍数、排名、倒计时、顶部记牌器数字都不会被当成牌。

牌的身份是 `点数+花色`（如 `AH`、`10S`、`BJ`、`SJ`），同一张牌只扣一次。底牌在顶部重复展示也会自动去重。

## 换游戏

新增一个游戏只需要：

1. 新建 `jjcounter/games/<id>/profile.py`，定义 `GameProfile`：
   - 窗口标题/应用名
   - `rank_totals`、`display_ranks`、`labels`
   - 识别模板目录、大小王模板/区域
   - 自动重置的手牌区域与阈值
2. 放入对应牌面模板到 `games/<id>/templates/`（或复用现有模板）；
3. 在 `games/registry.py` 注册；
4. 如果窗口/采集方式有平台差异，只改对应 `platforms/<platform>/`。

业务层（去重、计数、TUI、自动重置框架）不需要改。

## 换识别来源（例如内存读牌）

实现 `DetectionSource`：

```python
class MemoryDetectionSource(DetectionSource):
    def start(self): ...
    def read(self, timeout): return DetectionBatch(...)
    def stop(self): ...
    def stats(self): ...
```

把 `DetectionBatch.detections` 填成 `Detection(rank, suit, score, box)` 即可；`meta={"new_round": True}` 可以通知运行时重置。`core/deck.py`、`core/runtime.py`、`core/table.py` 全部复用。

## Windows 状态

- `platforms/windows/window.py` 已用 ctypes 实现窗口枚举（实验性，未在 Windows 实机验证）。
- `platforms/windows/capture.py` 是明确的 TODO：保留 `CaptureStream` 接口，后续接 Windows Graphics Capture / PrintWindow / ImageGrab。
- 在 Windows 上运行截图/实时记牌会得到明确的“后端尚未实现”错误，不会静默失败。

## 测试

```bash
.venv/bin/python -m unittest discover -s tests -v
```

覆盖：牌堆去重、帧内去重、自动重置正反例、真实截图识别、底牌大小王、窗口选择、游戏注册表、炸弹高亮。

> 开局时所有点数都可能有 4 张剩余，因此整排都会反色；这是“可能存在炸弹”的字面结果。
> 如果只想看少量高亮，后续可以增加“剩余牌总数低于阈值才高亮”的策略。

## 已知限制

- 窗口最小化后无法采集；完全遮挡时系统可能降低刷新率，建议保持游戏窗口可见。
- 程序从一局中间启动时，无法补回启动前已经出过的牌；请在发牌/开局前启动，或按 `r` 重置。
- 当前 JJ 牌堆按标准 54 张、每点数 4 张计算；特殊模式需要新 profile。
- 模板与游戏皮肤绑定；换字体/皮肤后运行：

  ```bash
  .venv/bin/python -m jjcounter.games.jj.build_templates
  ```

- Windows 截图尚未实现；macOS 需要屏幕录制权限。
