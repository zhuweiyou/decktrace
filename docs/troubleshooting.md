# 开发排障

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
| GUI 资源找不到 | 只拷贝了二进制，没拷贝 assets | 用 `cargo xtask package-macos` |
| 打包报重复游戏资源 | 两个游戏映射到同一目标目录 | 检查 crate 目录名是否等于 `GameProfile.id` |
| 游戏一直等待窗口 | 窗口标题/owner 不匹配 | 检查 `WindowSelector`，临时用 `--game <id>` 调试 |
| Windows 抓到黑屏 | PrintWindow/BitBlt 不支持硬件加速窗口 | 先让窗口前台可见；后续接 Windows Graphics Capture |
| Windows 冒烟无法运行 | Wine 对 x86_64 PE/版本兼容性问题 | 在真实 Windows 或 Windows CI 上跑 `windows-smoke` |

## 常用命令

```bash
cargo test --release --workspace
cargo run --release -p decktrace
cargo xtask package-macos
DECKTRACE_TIMING=1 cargo run --release -p decktrace
```
