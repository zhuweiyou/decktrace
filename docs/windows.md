# Windows 采集路线

## 当前状态

`crates/capture/src/windows.rs` 是实验实现，已在真实 Windows（11 22631）实测：

```text
Win32 窗口枚举
→ GetClientRect（进程设为 Per-Monitor-V2 DPI 感知，拿物理像素）
→ PrintWindow(PW_CLIENTONLY | PW_RENDERFULLCONTENT)
→ 失败退回屏幕 BitBlt
→ 直接画进可复用的 32 位自上而下 DIB section（不再 GetDIBits 二次拷贝）
→ BGRA → RGB（rayon 并行）→ RawFrame
```

关键点：

- **被遮挡/后台窗口**：`PrintWindow` 让窗口自己重绘到内存位图；实测整屏被另一不透明窗口遮挡时，
  仍能截到目标窗口的完整内容（微信整屏覆盖测试通过）。
- **局限**：部分硬件加速 / DirectX 独占全屏窗口 `PrintWindow` 可能返回黑屏；此时退回的屏幕
  `BitBlt` 只取屏幕可见像素，被遮挡会拿到上层画面。这类窗口需要 WGC（见下）。
- **DPI**：`PlatformImpl::new` 尽力设置 Per-Monitor-V2 感知，避免缩放显示器下客户区被虚拟化、
  截图被裁剪。
- **帧率**：`read()` 用「上一时间槽 + interval」推进，采集耗时不叠加进周期（否则 15fps 只能跑 ~12fps）。

已实测通过：

- 窗口模式：Chrome / 微信 / 设置(UWP) / 本程序 GUI 均能正确截图
- 被遮挡：整屏不透明覆盖下仍截到目标窗口内容
- 性能：1920×1152 单帧 ~17ms（连续抓帧约 60FPS）；15fps 节流实测 15.2FPS

验证命令：

```bash
cargo test -p capture --release                                     # 单元 + 真实窗口截图集成测试
cargo run --release -p capture --example windows-smoke -- "微信"     # 截一帧存 windows-smoke.png
cargo run --release -p capture --example fps-bench -- "微信" 120             # 连续抓帧吞吐
cargo run --release -p capture --example fps-bench -- "微信" 90 --paced 15  # 验证节流帧率
```

未完成：

- 全屏/独占全屏/硬件加速游戏窗口的 WGC 验证（GDI 可能黑屏）
- Windows 安装包

Wine 只能做有限冒烟测试；本机 Wine 无法启动该 x86_64 PE，不能据此判断可用。

## 为什么先做 GDI

| | GDI（当前） | WGC（正式目标） |
| --- | --- | --- |
| 实现复杂度 | 低 | 高：WinRT + D3D11 + DXGI + COM + 事件 |
| 系统要求 | 老 Windows 也能跑 | Windows 10 1903+ |
| 被遮挡窗口 | `PrintWindow` 通常可以 | 可以 |
| 硬件加速/全屏 | 容易黑屏 | 支持更好 |
| 性能 | CPU 拷贝，够 15fps | GPU 帧池，更稳 |
| 调试难度 | 低 | 高 |

当前阶段的目标是提供一个可编译、可替换的 Windows 基线，而不是把未验证的 WGC 标成完成。

## WGC 正式方案

### 依赖与模块

新增 Windows-only 模块，例如：

```text
crates/capture/src/windows/
  mod.rs          # 平台入口，WGC 优先，GDI fallback
  wgc.rs          # Windows Graphics Capture
  gdi.rs          # 当前 PrintWindow/BitBlt 实现
```

需要的 `windows` crate features（以实际编译为准）：

```text
Win32_Foundation
Win32_Graphics_Direct3D11
Win32_Graphics_Dxgi
Win32_Graphics_Dxgi_Common
Win32_Graphics_Gdi
Win32_System_Com
Win32_System_Threading
Win32_UI_WindowsAndMessaging
Graphics_Capture
Graphics_DirectX_Direct3D11
Graphics_Dxgi
System_WinRT
```

### 实现步骤

1. **创建 D3D11 设备**
   - `D3D11CreateDevice`
   - 必须支持 BGRA
   - 创建 `ID3D11Device` / `ID3D11DeviceContext`

2. **创建 WGC capture item**
   - 从窗口 HWND 创建 `GraphicsCaptureItem`
   - 创建 `GraphicsCaptureSession`
   - 设置 `IsCursorCaptureEnabled(false)`
   - 视情况设置 `IsBorderRequired(false)`（需要系统支持）

3. **创建 frame pool**
   - `Direct3D11CaptureFramePool::CreateFreeThreaded`
   - 像素格式 `B8G8R8A8UIntNormalized`
   - 注册 `FrameArrived`

4. **接收帧**
   - 回调中 `TryGetNextFrame`
   - 获取 `IDirect3DSurface` / D3D11 texture
   - 复制到 staging texture
   - `Map` 读取 BGRA
   - 转成 `RgbImage`

5. **处理边界情况**
   - 窗口 resize：重建 frame pool
   - D3D device lost：重建设备和 session
   - session closed：重新创建
   - 窗口最小化：暂停/报错
   - 事件线程生命周期和 COM 初始化
   - `Send/Sync`：回调与 Session worker 之间只传 `RawFrame`

6. **运行时选择**

```text
尝试 WGC
  → 成功：使用 WGC
  → 失败/不支持：回退 GDI PrintWindow/BitBlt
```

必须在真实 Windows 上验证，不要靠 `cargo check` 判断 WGC 可用。

## 验证矩阵

- [ ] 窗口模式
- [ ] 无边框窗口
- [ ] 全屏窗口
- [ ] 硬件加速窗口
- [ ] 窗口被遮挡
- [ ] 最小化/恢复
- [ ] 连续运行 30 分钟
- [ ] 1080p / 2K / 高 DPI
- [ ] Windows 10 1903+
- [ ] Windows 11

## xtask 计划

Windows 打包属于后续工作，计划增加：

```bash
cargo xtask package-windows
```

职责：

- 在 Windows 原生构建，或在非 Windows 主机交叉构建 `x86_64-pc-windows-gnu`
- 生成 `dist/windows/decktrace/`
- 拷贝：
  - `decktrace.exe`
  - `assets/<game>/templates`
  - `assets/cards`
- 生成启动脚本/说明
- 安装包（MSI/NSIS）后续再接

当前只有：

```bash
cargo xtask package-macos
```

## 冒烟测试

```bash
cargo run --target x86_64-pc-windows-gnu -p capture \
  --example windows-smoke -- "窗口标题"
```

这是开发冒烟，不是验收。
