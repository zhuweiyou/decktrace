# 新增游戏接入指南

本文以新增「欢乐斗地主」（ID: `hl`）为例。目标是新增一个 `hl` crate，
提供 `GameProfile` 和 `Recognizer`，然后注册到应用。

采集、`Session`、GUI 不需要为单个游戏修改。

## 0. 自动检测行为

应用默认 `--game auto`：启动后扫描当前窗口，逐个匹配已注册游戏的 `WindowSelector`。

- 只开一个支持的游戏窗口：自动进入该游戏
- 同时开多个：按 `list_games()` 注册顺序选第一个，并在 stderr 打印提示
- 一个都没开：GUI 显示 `Waiting for game window...`，每 500ms 重试
- 调试/强制指定：`--game jj`、`--game hl`

新增游戏后只需要保证它出现在 `list_games()` 和 `create_recognizer()` 里。

## 1. 创建 crate

在根 `Cargo.toml` 的 `workspace.members` 增加：

```toml
"crates/hl",
```

新建 `crates/hl/Cargo.toml`：

```toml
[package]
name = "hl"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
tracking = { path = "../tracking" }
image.workspace = true
imageproc.workspace = true
rayon.workspace = true
thiserror.workspace = true
```

新建 `crates/hl/src/lib.rs`：

```rust
pub mod profile;
pub mod recognition;

pub use profile::profile;
pub use recognition::HlRecognizer;
```

目录最终类似：

```text
crates/hl/
  Cargo.toml
  src/
    lib.rs
    profile.rs
    recognition.rs
  assets/
    templates/
      ranks/*.png
      suits/*.png
      joker.png
      joker_compact.png
    cards/*.png
```

## 2. 写 GameProfile

`src/profile.rs` 以 `jj/src/profile.rs` 为模板，核心是：

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tracking::{
    BottomConfig, GameProfile, JokerBand, RecognitionConfig, ResetConfig, WindowSelector,
};

fn asset_root() -> PathBuf {
    if let Ok(root) = std::env::var("DECKTRACE_ASSETS") {
        return root.join("hl");
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents) = exe.parent().and_then(|macos| macos.parent()) {
            let bundled = contents.join("Resources/assets/hl");
            if bundled.join("templates").exists() {
                return bundled;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

pub fn profile() -> GameProfile {
    let assets = asset_root();

    let mut rank_totals = BTreeMap::new();
    for (rank, total) in [
        ("BJ", 1u8), ("SJ", 1), ("2", 4), ("A", 4), ("K", 4),
        ("Q", 4), ("J", 4), ("10", 4), ("9", 4), ("8", 4),
        ("7", 4), ("6", 4), ("5", 4), ("4", 4), ("3", 4),
    ] {
        rank_totals.insert(rank.to_string(), total);
    }

    let mut labels = BTreeMap::new();
    labels.insert("BJ".to_string(), "大王".to_string());
    labels.insert("SJ".to_string(), "小王".to_string());

    let display_ranks = [
        "BJ", "SJ", "2", "A", "K", "Q", "J", "10",
        "9", "8", "7", "6", "5", "4", "3",
    ].into_iter().map(str::to_string).collect();

    GameProfile {
        id: "hl".to_string(),
        name: "欢乐斗地主".to_string(),
        window: WindowSelector::new("欢乐斗地主"),

        templates_dir: assets.join("templates"),
        card_assets_dir: assets.join("cards"),

        rank_totals,
        display_ranks,
        labels,
        suits: ["H", "D", "S", "C"].into_iter().map(str::to_string).collect(),

        recognition: RecognitionConfig {
            rank_template_height: 70,
            reference_width: 1600,
            joker_template: "joker.png".to_string(),
            joker_compact_template: Some("joker_compact.png".to_string()),
            joker_bands: vec![
                JokerBand {
                    top_ratio: 0.00,
                    bottom_ratio: 0.22,
                    template: "joker_compact.png".to_string(),
                    scales: vec![0.90, 1.00, 1.10],
                },
                JokerBand {
                    top_ratio: 0.22,
                    bottom_ratio: 0.50,
                    template: "joker.png".to_string(),
                    scales: vec![0.54, 0.60, 0.66, 0.72],
                },
                JokerBand {
                    top_ratio: 0.62,
                    bottom_ratio: 1.00,
                    template: "joker.png".to_string(),
                    scales: vec![0.92, 1.00, 1.08],
                },
            ],
            ..RecognitionConfig::default()
        },

        reset: ResetConfig {
            hand_top_ratio: 0.60,
            hand_bottom_ratio: 0.70,
            ..ResetConfig::default()
        },

        bottom: Some(Arc::new(BottomConfig::new(
            (0.33, 0.08, 0.47, 0.20),
            3,
            2,
        ))),
    }
}
```

需要按 HL 实际截图调整：

- `window`：窗口标题，必要时设置 `WindowSelector.app`
- `reference_width`：调大小王尺度时的窗口宽度
- `joker_bands`：底牌栏、出牌区、手牌区的高度比例
- `BottomConfig`：底牌区域 `(x0, y0, x1, y1)`，归一化 0–1
- `ResetConfig`：手牌区域比例

没有底牌展示的游戏：`bottom: None`。

## 3. 准备素材

### 牌面 `cards/`

GUI 显示用，15×30 PNG，命名规则：

```text
3C.png 3D.png 3H.png 3S.png
...
10C.png 10D.png 10H.png 10S.png
JC.png ... AS.png
BJ.png SJ.png
```

`all_card_ids()` 按 `display_ranks` + `suits` 生成 ID，`BJ/SJ` 单独处理。

### 模板 `templates/ranks/`

- 点数 glyph，灰度 PNG
- 白色字形 + 黑色背景
- 紧贴内容裁剪
- 高度统一 70px，宽度按比例
- 文件名就是点数：`3.png`、`10.png`、`A.png`、`K.png` 等

识别时会先把候选点数缩放到高度 70，再缩放到每个模板尺寸做 IoU。

### 模板 `templates/suits/`

- `H.png`、`D.png`、`S.png`、`C.png`
- 40×40 画布，白字黑底
- 字形成分居中

### 大小王模板

- `joker.png`：正常大小王字形
- `joker_compact.png`：底牌栏压缩字形，可选

制作流程：

1. 截一张完整 HL 窗口
2. 裁出点数/花色/大小王区域
3. 二值化成白字黑底
4. 紧贴内容裁剪
5. 按上面尺寸保存

## 4. 实现 Recognizer

第一版最快方式：复制 `crates/jj/src/recognition.rs` 到
`crates/hl/src/recognition.rs`，然后：

- `JjRecognizer` → `HlRecognizer`
- 保留 `Scratch`、RLE 连通域、模板 IoU、NMS、大小王搜索
- 调整 `masks()` 的红/黑阈值：

```rust
let is_red = r > 110 && (r as i32 - g as i32) > 45 && (r as i32 - b as i32) > 35;
let is_dark = r < 90 && g < 90 && b < 90;
```

- 如果 HL 的卡面亮度不同，调整 `face_min_fraction`
- 如果 HL 的模板/字号不同，调整 `RecognitionConfig`

最后实现 trait：

```rust
impl Recognizer for HlRecognizer {
    fn recognize(&self, image: &RgbImage, detect_jokers: bool) -> RecognitionResult {
        HlRecognizer::recognize(self, image, detect_jokers)
    }
}
```

> 识别实现属于游戏适配层。两个游戏阶段先各维护一份；等算法和参数稳定后，
> 再考虑抽公共算法模块，但游戏模板、素材、阈值仍应留在各自 crate。

## 5. 注册游戏

`crates/decktrace/src/registry.rs`：

```rust
pub fn get_game(id: &str) -> Result<GameProfile, String> {
    match id {
        "jj" => {
            let game = jj::profile();
            game.validate()?;
            Ok(game)
        }
        "hl" => {
            let game = hl::profile();
            game.validate()?;
            Ok(game)
        }
        other => Err(format!("未知游戏：{other}")),
    }
}

pub fn list_games() -> Vec<GameProfile> {
    vec![
        jj::profile(),
        hl::profile(),
    ]
}

pub fn create_recognizer(game: &GameProfile) -> Result<SharedRecognizer, String> {
    match game.id.as_str() {
        "jj" => Ok(Arc::new(
            jj::JjRecognizer::new(game).map_err(|e| e.to_string())?,
        )),
        "hl" => Ok(Arc::new(
            hl::HlRecognizer::new(game).map_err(|e| e.to_string())?,
        )),
        other => Err(format!("没有为游戏 {other} 注册识别器")),
    }
}
```

给 `crates/decktrace/Cargo.toml` 加：

```toml
hl = { path = "../hl" }
```

`main.rs` 不需要改：它通过 `list_games()` 自动检测窗口，通过
`create_recognizer()` 创建对应识别器。

## 6. 打包资源

`cargo xtask package-macos` 会自动扫描：

```text
crates/*/assets
```

规则：

- 只有包含 `src/profile.rs` 的游戏 crate 才会被处理
- crate 目录名必须等于 `GameProfile.id`，例如 `hl` 对应 `id = "hl"`
- 每个游戏单独拷到 `Contents/Resources/assets/<game>/`，不会互相覆盖
- 如果目标目录重复，脚本会直接报错退出

新增游戏时通常不需要改打包脚本，只要遵守上面的目录约定。

最终：

```text
Contents/Resources/assets/
  jj/
  hl/
```

`asset_root()` 会按 `DECKTRACE_ASSETS/hl` → `.app/Contents/Resources/assets/hl`
→ 开发目录的顺序查找。

## 7. 测试

新增 `crates/hl/tests/assets.rs`：

```rust
use hl::profile;

#[test]
fn assets_are_complete() {
    let game = profile();
    game.validate().expect("profile");

    for card_id in game.all_card_ids() {
        let path = game.card_assets_dir.join(format!("{card_id}.png"));
        assert!(path.exists(), "缺少牌面素材：{}", path.display());
    }

    assert!(game.templates_dir.join("ranks").is_dir());
    assert!(game.templates_dir.join("suits").is_dir());
}
```

再加真实截图回归，参照 `crates/jj/tests/golden.rs`：

- 截图放 `shots/hl/`
- 使用 `HlRecognizer` 识别
- 断言检测到的牌集合和底牌集合

运行：

```bash
cargo test --release --workspace
cargo run --release -p decktrace          # 自动检测
cargo run --release -p decktrace -- --game hl
DECKTRACE_TIMING=1 cargo run --release -p decktrace
```

## 8. 调参顺序

1. 窗口能稳定找到
2. GUI 能显示 54 张牌
3. 静态截图能正确识别点数和花色
4. 大小王能正确识别
5. 底牌区域和 3 张门控正确
6. 自动重置正确
7. 性能保持在 15 FPS 下有足够余量

## 9. 检查清单

- [ ] `cargo test --release --workspace` 通过
- [ ] `get_game` / `list_games` / `create_recognizer` 已注册
- [ ] `assets` 路径在开发环境和 `.app` 内都能找到
- [ ] 54 张牌面齐全
- [ ] 模板尺寸/二值化符合要求
- [ ] 底牌区域和重置比例用真实截图调过
- [ ] golden 回归覆盖至少一局真实画面
- [ ] `cargo xtask package-macos` 拷贝了新游戏资源
