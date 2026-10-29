# 识别管线与调参

## 流程

```text
red/dark 掩码
→ 连通域
→ 花色候选过滤
→ 花色上方定位点数
→ 点数/花色模板 IoU
→ 卡面亮度校验
→ 同一帧 NMS
→ 大小王搜索
→ 底牌标记
```

## 掩码与连通域

- red 与 dark 一次并行扫描，同时产出 combined。
- red 阈值因游戏而异：JJ 为 `r > 110 && r-g > 45 && r-b > 35`；王者斗地主(wzddz) 用 `r > 100 && r-g > 30 && r-b > 25`。
- dark 阈值目前是 `r,g,b < 90`。
- 不同游戏皮肤差异大时，在游戏自己的 `recognition.rs` 中调整。
- 连通域使用行程编码（RLE）+ 两趟并查集。
- 修改连通域后必须跑 `recognition.rs` 内的 `tests` 的随机 mask 对拍。

## 模板格式

### 点数模板 `templates/ranks/`

- 灰度 PNG，白色字形、黑色背景
- 紧贴内容裁剪
- 高度统一 70px，宽度按比例（王者斗地主用**固定 32×40 画布**，消除不同牌宽高比造成的形变）
- 文件名即点数：`3.png`、`10.png`、`A.png`、`K.png`

候选点数先缩放到高度 70，再缩放到每个模板尺寸做 IoU。

### 花色模板 `templates/suits/`

- `H.png`、`D.png`、`S.png`、`C.png`
- 40×40 画布，字形居中
- 白字黑底

### 大小王模板

- `joker.png`：正常大小王字形
- `joker_compact.png`：底牌栏压缩字形，可选

## 大小王搜索

- 按 `RecognitionConfig.joker_bands` 的高度比例裁区域。
- 每个高度带按 `scales` 做多尺度搜索。
- 先缩小图 CCOEFF 选候选，再全分辨率局部精验。
- 双王都识别后，运行时跳过后续王搜索。

## 底牌门控

- 底牌识别是游戏能力：`GameProfile.bottom: Option<Arc<dyn BottomSpec>>`。
- JJ斗地主使用 `BottomConfig(region=(0.33, 0.08, 0.47, 0.20), count=3, stable_frames=2)`。
- `CardTracker` 在 `armed=false` 时忽略所有排除，只收集底牌区检测。
- 连续 `stable_frames` 帧出现同样的 `count` 张底牌后开局。
- 开局那一帧跳过底牌动画残影。
- 底牌仍计入剩余；真正打出后才从 `bottom_ids` 移除。
- 没有底牌概念的游戏设 `bottom=None`，tracker 一开始就是 armed。
- 王者斗地主(wzddz) 即 `bottom=None`，开局由 JNI 侧「手牌发齐 + 底牌亮卡」门控。

## 王者斗地主（wzddz）

手机横屏、参考宽度见 `crates/wzddz/src/profile.rs`。要点：

- **掩码/连通域**：red `r>100 && r-g>30 && r-b>25`、dark `r,g,b<90`；自研 RLE+并查集。
- **点数定位**：以花色连通域为锚点，`rank_above` 取上方点数块，取点数窗口**按牌高自适应**（不写死像素，适配不同分辨率）。
- **归一化**：点数固定 32×40、花色 40×40 后再做模板 IoU。
- **点数裁切提纯**：按行投影取“最长连续字形带”，剔除最后一张牌身份水印/飘带等杂点。
- **王**：竖排 `JOKER` 检测 + “细长竖列(高≥宽×4)” + 白卡占比，排除角色服装花纹误判。
- **结算/大厅**：`imageproc::template_matching` 匹配「退出」「继续游戏」「开始匹配」按钮，连续 3 帧防抖后结束本局。
- **白卡底校验**：普通牌要求点数块与花色块都落在白底上（排除积分/特效/背景误检）。
- **开局门控**：`bottom=None`，改由 JNI 判断「手牌发齐(≥13) + 底牌亮卡出现」开始记牌。

## 欢乐斗地主的差异

`hlddz` 的牌面与 JJ 不同，`crates/hlddz/src/recognition.rs` 里单独处理：

- **大小王没有“王”字**：牌面是竖排 `JOKER` 字母。按同列字母聚类识别，
  红色=大王（`BJ`）、黑色=小王（`SJ`）。
- **底牌常驻顶部**：底牌会一直显示在牌桌顶部，已打出后变成半透明。
  半透明字形的对比度很低，单独走“按每张牌自身亮度分布取阈值”的通路。
- **记牌器面板不是牌**：顶部可展开的记牌器（大王…3 + 张数）是游戏内购功能，
  展开时会把底牌顶到右边；数字不能被当成牌面入账。
- **托管遮罩**：开启托管后整块手牌会压暗，红色饱和度下降；
  `red` 掩码降到 `r>100 && r-g>30 && r-b>25`，卡面亮度门限降到 `mx>120`。

## 性能

release、约 1800×1100：

- 无王搜索：约 8–10ms/帧
- 含王搜索：约 16–25ms/帧
- 双王都落地后自动跳过王搜索

调试：

```bash
DECKTRACE_TIMING=1 cargo run --release -p app   # 单帧耗时
DECKTRACE_TRACE=1 cargo run --release -p app    # 入账/底牌/重置逐条打印
```

`DECKTRACE_TRACE` 只用于调试：默认关闭，正式运行时不要设置。
它会在状态变化时向 stderr 打印 `armed / seen / remaining / new / bottoms`。

## 回归测试

- 连通域随机 mask 对拍：`recognition.rs` 内的 `tests`
- 真实截图 golden：`crates/jjddz/tests/golden.rs`
- 样本目录：根目录 `shots/`（本地目录，不入库）
- 样本缺失时测试自动 skip

新增游戏模板时，同时补该游戏的 golden 测试。
