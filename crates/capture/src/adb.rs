//! ADB 采集后端：通过 `adb exec-out screencap -p` 抓取安卓设备屏幕。
//!
//! 仅用于桌面端联调（阶段一）：电脑上运行本程序，adb 连接手机，把手机画面
//! 当作一个“窗口”送进现有识别/记牌管线。先验证识别能跑通，再上端上
//! MediaProjection 方案（见 `docs/android.md`）。
//!
//! 相关环境变量：
//! - `DECKTRACE_ADB_SERIAL` / `ANDROID_SERIAL`：指定设备序列号（多设备时）。
//! - `DECKTRACE_ADB_CROP`：截图裁剪区域 `x,y,w,h`（截图像素）。
//! - `DECKTRACE_ADB_BIN`：adb 可执行文件路径（默认走 PATH 里的 `adb`）。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use image::RgbImage;

use crate::traits::{CaptureError, CaptureStream, Platform, Result, WindowService};
use crate::types::{RawFrame, WindowInfo};

#[derive(Clone, Copy, Debug)]
struct Crop {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// 通过 adb 采集安卓设备的采集后端。
pub struct AdbPlatform {
    serial: Option<String>,
    crop: Option<Crop>,
    windows: AdbWindowService,
}

impl AdbPlatform {
    /// `crop` 形如 `x,y,w,h`，为空则整屏。
    pub fn new(serial: Option<String>, crop: Option<String>) -> Result<Self> {
        let crop = match crop.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(text) => Some(parse_crop(text)?),
            None => None,
        };
        let serial = serial.filter(|s| !s.trim().is_empty());
        Ok(Self {
            windows: AdbWindowService {
                serial: serial.clone(),
                crop,
            },
            serial,
            crop,
        })
    }

    /// 从环境变量读取配置。
    pub fn from_env() -> Result<Self> {
        let serial = std::env::var("DECKTRACE_ADB_SERIAL")
            .ok()
            .or_else(|| std::env::var("ANDROID_SERIAL").ok())
            .filter(|s| !s.trim().is_empty());
        let crop = std::env::var("DECKTRACE_ADB_CROP").ok();
        Self::new(serial, crop)
    }
}

impl Platform for AdbPlatform {
    fn name(&self) -> &str {
        "adb"
    }

    fn window_service(&self) -> &dyn WindowService {
        &self.windows
    }

    fn create_capture_stream(
        &self,
        _window: &WindowInfo,
        fps: f32,
    ) -> Result<Box<dyn CaptureStream>> {
        Ok(Box::new(AdbCaptureStream::new(
            self.serial.clone(),
            self.crop,
            fps,
        )))
    }
}

struct AdbWindowService {
    serial: Option<String>,
    crop: Option<Crop>,
}

impl WindowService for AdbWindowService {
    fn list_windows(&self, _include_off_screen: bool) -> Result<Vec<WindowInfo>> {
        let size_text = adb_text(&self.serial, &["shell", "wm", "size"])?;
        let (mut width, mut height) = parse_wm_size(&size_text).ok_or_else(|| {
            CaptureError::Window(format!("无法解析 `wm size` 输出：{size_text:?}"))
        })?;
        let (x, y) = match self.crop {
            Some(crop) => {
                width = crop.w;
                height = crop.h;
                (crop.x as f64, crop.y as f64)
            }
            None => (0.0, 0.0),
        };
        let app = focused_package(&self.serial).unwrap_or_else(|| "android".to_string());
        Ok(vec![WindowInfo {
            id: 1,
            title: "Android Device".to_string(),
            app,
            pid: -1,
            x,
            y,
            width: width as f64,
            height: height as f64,
            onscreen: true,
            layer: 0,
        }])
    }
}

struct AdbCaptureStream {
    serial: Option<String>,
    crop: Option<Crop>,
    seq: u64,
    started: Instant,
    next_at: Instant,
    interval: Duration,
    logged_size: bool,
}

impl AdbCaptureStream {
    fn new(serial: Option<String>, crop: Option<Crop>, fps: f32) -> Self {
        let fps = if fps.is_finite() && fps > 0.0 { fps } else { 5.0 };
        let now = Instant::now();
        Self {
            serial,
            crop,
            seq: 0,
            started: now,
            next_at: now,
            interval: Duration::from_secs_f64(1.0 / fps as f64),
            logged_size: false,
        }
    }
}

impl CaptureStream for AdbCaptureStream {
    fn start(&mut self) -> Result<()> {
        // 提前校验设备在线，给出比“截图失败”更清楚的提示。
        let out = adb_output(&self.serial, &["shell", "wm", "size"])?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(CaptureError::Capture(format!(
                "adb 未连接设备或无法执行 wm size：{}\n提示：先运行 `adb devices` 确认设备在线并授权调试。",
                err.trim()
            )));
        }
        self.started = Instant::now();
        self.next_at = self.started;
        self.seq = 0;
        Ok(())
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        // 按目标帧率节流；单次 screencap 经 USB 通常 150–400ms，实际 fps 以抓取耗时为下限。
        let now = Instant::now();
        if now < self.next_at {
            let wait = (self.next_at - now).min(timeout);
            if wait > Duration::ZERO {
                std::thread::sleep(wait);
            }
            if Instant::now() < self.next_at {
                return Ok(None);
            }
        }

        let read_started = Instant::now();
        let out = adb_output(&self.serial, &["exec-out", "screencap", "-p"])?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(CaptureError::Capture(format!(
                "screencap 失败：{}",
                err.trim()
            )));
        }
        if out.stdout.is_empty() {
            return Err(CaptureError::Capture("screencap 返回空数据".to_string()));
        }
        let image = image::load_from_memory(&out.stdout)
            .map_err(|err| CaptureError::Capture(format!("解码截图 PNG 失败：{err}")))?;
        let rgb = image.to_rgb8();
        let image = match self.crop {
            Some(crop) => crop_image(rgb, crop)?,
            None => rgb,
        };
        let (width, height) = image.dimensions();
        if !self.logged_size {
            eprintln!(
                "[adb] 截图 {}x{}，单帧抓取 {:.0}ms",
                width,
                height,
                read_started.elapsed().as_secs_f64() * 1000.0
            );
            self.logged_size = true;
        }
        self.seq += 1;
        self.next_at = read_started + self.interval;
        Ok(Some(RawFrame {
            seq: self.seq,
            timestamp: self.started.elapsed().as_secs_f64(),
            image,
            width,
            height,
        }))
    }

    fn stop(&mut self) {}
}

fn crop_image(image: RgbImage, crop: Crop) -> Result<RgbImage> {
    let (width, height) = image.dimensions();
    if crop.x >= width || crop.y >= height {
        return Err(CaptureError::Capture(format!(
            "裁剪区域超出截图范围：截图 {width}x{height}，区域 {}x{}+{}+{}",
            crop.w, crop.h, crop.x, crop.y
        )));
    }
    let w = crop.w.min(width - crop.x);
    let h = crop.h.min(height - crop.y);
    if w == 0 || h == 0 {
        return Err(CaptureError::Capture(format!(
            "裁剪区域超出截图范围：截图 {width}x{height}，区域 {}x{}+{}+{}",
            crop.w, crop.h, crop.x, crop.y
        )));
    }
    Ok(image::imageops::crop_imm(&image, crop.x, crop.y, w, h).to_image())
}

fn adb_bin() -> String {
    std::env::var("DECKTRACE_ADB_BIN").unwrap_or_else(|_| "adb".to_string())
}

fn adb_command(serial: &Option<String>) -> Command {
    let mut cmd = Command::new(adb_bin());
    if let Some(serial) = serial.as_ref().filter(|s| !s.trim().is_empty()) {
        cmd.arg("-s").arg(serial);
    }
    cmd
}

fn adb_output(serial: &Option<String>, args: &[&str]) -> Result<std::process::Output> {
    adb_command(serial)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| {
            CaptureError::Capture(format!(
                "调用 adb 失败（是否安装并加入 PATH？可设 DECKTRACE_ADB_BIN）：{err}"
            ))
        })
}

fn adb_text(serial: &Option<String>, args: &[&str]) -> Result<String> {
    let out = adb_output(serial, args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(CaptureError::Capture(format!(
            "adb {:?} 失败：{}",
            args,
            err.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_wm_size(text: &str) -> Option<(u32, u32)> {
    let mut physical = None;
    let mut overridden = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let dims = parse_dims(value.trim());
        match key.trim().to_ascii_lowercase().as_str() {
            "physical size" => physical = dims,
            "override size" => overridden = dims,
            _ => {}
        }
    }
    overridden.or(physical)
}

fn parse_dims(value: &str) -> Option<(u32, u32)> {
    let (w, h) = value.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn parse_crop(text: &str) -> Result<Crop> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(CaptureError::Platform(format!(
            "裁剪区域需要 x,y,w,h 四段，收到 {text:?}"
        )));
    }
    let mut values = [0u32; 4];
    for (index, part) in parts.iter().enumerate() {
        values[index] = part.parse().map_err(|_| {
            CaptureError::Platform(format!("裁剪区域第 {} 段不是整数：{part:?}", index + 1))
        })?;
    }
    if values[2] == 0 || values[3] == 0 {
        return Err(CaptureError::Platform(
            "裁剪区域宽高必须大于 0".to_string(),
        ));
    }
    Ok(Crop {
        x: values[0],
        y: values[1],
        w: values[2],
        h: values[3],
    })
}

fn focused_package(serial: &Option<String>) -> Option<String> {
    let text = adb_text(serial, &["shell", "dumpsys", "window"]).ok()?;
    for line in text.lines() {
        if !(line.contains("mCurrentFocus") || line.contains("mFocusedApp")) {
            continue;
        }
        for token in line.split(|c: char| c.is_whitespace() || c == '{' || c == '}' || c == '=') {
            let Some((package, _activity)) = token.split_once('/') else {
                continue;
            };
            if package.contains('.') && !package.starts_with("u0") {
                return Some(package.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wm_size_prefers_override() {
        let text = "Physical size: 1080x2400\nOverride size: 720x1600\n";
        assert_eq!(parse_wm_size(text), Some((720, 1600)));
        assert_eq!(parse_wm_size("Physical size: 1080x2400\n"), Some((1080, 2400)));
        assert_eq!(parse_wm_size("garbage"), None);
    }

    #[test]
    fn parses_crop() {
        let crop = parse_crop("10, 20, 1920, 1080").expect("valid crop");
        assert_eq!((crop.x, crop.y, crop.w, crop.h), (10, 20, 1920, 1080));
        assert!(parse_crop("10,20,1920").is_err());
        assert!(parse_crop("0,0,0,1080").is_err());
    }

    #[test]
    fn crops_within_bounds() {
        let image = RgbImage::new(100, 50);
        let cropped = crop_image(image.clone(), Crop { x: 10, y: 10, w: 200, h: 100 }).expect("clamped");
        assert_eq!(cropped.dimensions(), (90, 40));
        assert!(crop_image(image, Crop { x: 200, y: 0, w: 10, h: 10 }).is_err());
    }
}
