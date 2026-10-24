//! macOS ScreenCaptureKit 实现。窗口枚举与帧回调都在 Rust 进程内完成。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{CaptureError, CaptureStream, Platform, RawFrame, Result, WindowInfo, WindowService};
use crossbeam_channel::{Receiver, Sender, bounded};
use image::RgbImage;
use screencapturekit::cm::CMSampleBufferExt;
use screencapturekit::cv::CVPixelBufferLockFlags;
use screencapturekit::prelude::*;
use screencapturekit::shareable_content::SCShareableContentInfo;

pub struct MacPlatform {
    windows: MacWindowService,
}

impl MacPlatform {
    pub fn new() -> Result<Self> {
        init_coregraphics();
        Ok(Self {
            windows: MacWindowService,
        })
    }
}

impl Platform for MacPlatform {
    fn name(&self) -> &str {
        "macos"
    }

    fn window_service(&self) -> &dyn WindowService {
        &self.windows
    }

    fn create_capture_stream(
        &self,
        window: &WindowInfo,
        fps: f32,
    ) -> Result<Box<dyn CaptureStream>> {
        Ok(Box::new(SckCaptureStream::new(window, fps)?))
    }
}

pub struct MacWindowService;

impl WindowService for MacWindowService {
    fn list_windows(&self, include_off_screen: bool) -> Result<Vec<WindowInfo>> {
        let content = SCShareableContent::create()
            .with_on_screen_windows_only(!include_off_screen)
            .with_exclude_desktop_windows(true)
            .get()
            .map_err(|err| {
                CaptureError::Platform(format!(
                    "{err}；请确认已授予屏幕录制权限（系统设置 → 隐私与安全性 → 屏幕与系统音频录制）"
                ))
            })?;

        let mut windows = Vec::new();
        for window in content.windows() {
            let title = window.title().unwrap_or_default();
            let app = window
                .owning_application()
                .map(|application| application.application_name())
                .unwrap_or_default();
            let pid = window
                .owning_application()
                .map(|application| application.process_id())
                .unwrap_or(0);
            let frame = window.frame();
            windows.push(WindowInfo {
                id: window.window_id(),
                title,
                app,
                pid,
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
                onscreen: window.is_on_screen(),
                layer: window.window_layer(),
            });
        }
        Ok(windows)
    }
}

struct FrameHandler {
    tx: Sender<RawFrame>,
    rx: Receiver<RawFrame>,
    seq: Arc<AtomicU64>,
}

impl SCStreamOutputTrait for FrameHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, output_type: SCStreamOutputType) {
        if !matches!(output_type, SCStreamOutputType::Screen) {
            return;
        }
        let Some(pixel_buffer) = sample.pixel_buffer() else {
            return;
        };
        let Ok(guard) = pixel_buffer.lock(CVPixelBufferLockFlags::READ_ONLY) else {
            return;
        };
        let width = guard.width() as u32;
        let height = guard.height() as u32;
        let bytes_per_row = guard.bytes_per_row() as usize;
        let Some(slice) = (unsafe { guard.as_slice() }) else {
            return;
        };
        let Some(image) = bgra_to_rgb(slice, width, height, bytes_per_row) else {
            return;
        };
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let frame = RawFrame {
            seq,
            timestamp,
            image,
            width,
            height,
        };
        if self.tx.try_send(frame.clone()).is_err() {
            let _ = self.rx.try_recv();
            let _ = self.tx.try_send(frame);
        }
    }
}

pub struct SckCaptureStream {
    stream: Option<SCStream>,
    rx: Receiver<RawFrame>,
    started: bool,
}

impl SckCaptureStream {
    fn new(window: &WindowInfo, fps: f32) -> Result<Self> {
        let content = SCShareableContent::get().map_err(|err| {
            CaptureError::Platform(format!(
                "{err}；请确认已授予屏幕录制权限（系统设置 → 隐私与安全性 → 屏幕与系统音频录制）"
            ))
        })?;
        let windows = content.windows();
        let live = windows
            .iter()
            .find(|candidate| candidate.window_id() == window.id)
            .ok_or_else(|| CaptureError::Window(format!("窗口已关闭：{}", window.id)))?;
        let filter = SCContentFilter::create()
            .with_window(live)
            .build()
            .map_err(|err| CaptureError::Capture(err.to_string()))?;

        let scale = SCShareableContentInfo::for_filter(&filter)
            .map(|info| info.point_pixel_scale())
            .unwrap_or(1.0);
        let pixel_width = (window.width * scale as f64).round().max(1.0) as u32;
        let pixel_height = (window.height * scale as f64).round().max(1.0) as u32;

        let config = SCStreamConfiguration::new()
            .with_width(pixel_width)
            .with_height(pixel_height)
            .with_pixel_format(PixelFormat::BGRA)
            .with_fps(fps.max(1.0).round() as u32)
            .with_queue_depth(3)
            .with_shows_cursor(false);

        let mut stream = SCStream::new(&filter, &config)
            .map_err(|err| CaptureError::Capture(err.to_string()))?;
        let (tx, rx) = bounded(3);
        let handler = FrameHandler {
            tx,
            rx: rx.clone(),
            seq: Arc::new(AtomicU64::new(0)),
        };
        stream
            .add_output_handler(handler, SCStreamOutputType::Screen)
            .map_err(|err| CaptureError::Capture(err.to_string()))?;

        Ok(Self {
            stream: Some(stream),
            rx,
            started: false,
        })
    }
}

impl CaptureStream for SckCaptureStream {
    fn start(&mut self) -> Result<()> {
        if let Some(stream) = &self.stream {
            stream
                .start_capture()
                .map_err(|err| CaptureError::Capture(err.to_string()))?;
        }
        self.started = true;
        Ok(())
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        match self.rx.recv_timeout(timeout) {
            Ok(frame) => Ok(Some(frame)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(None),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                Err(CaptureError::Capture("采集通道已断开".to_string()))
            }
        }
    }

    fn stop(&mut self) {
        if self.started {
            if let Some(stream) = &self.stream {
                let _ = stream.stop_capture();
            }
            self.started = false;
        }
    }

    fn backend_name(&self) -> &str {
        "screencapturekit"
    }
}

impl Drop for SckCaptureStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// BGRA/BGRx 像素缓冲转 RGB。按行预分配写入，避免逐像素 push。
fn bgra_to_rgb(slice: &[u8], width: u32, height: u32, bytes_per_row: usize) -> Option<RgbImage> {
    let row_bytes = width as usize * 4;
    if width == 0 || height == 0 || bytes_per_row < row_bytes {
        return None;
    }
    let mut data = vec![0u8; width as usize * height as usize * 3];
    for row in 0..height as usize {
        let start = row * bytes_per_row;
        let end = start + row_bytes;
        if end > slice.len() {
            return None;
        }
        let src = &slice[start..end];
        let dst = &mut data[row * width as usize * 3..(row + 1) * width as usize * 3];
        for (dst_px, src_px) in dst.chunks_exact_mut(3).zip(src.chunks_exact(4)) {
            dst_px[0] = src_px[2];
            dst_px[1] = src_px[1];
            dst_px[2] = src_px[0];
        }
    }
    RgbImage::from_raw(width, height, data)
}

fn init_coregraphics() {
    unsafe extern "C" {
        fn sc_initialize_core_graphics();
    }
    unsafe { sc_initialize_core_graphics() }
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgb;

    #[test]
    fn bgra_to_rgb_converts_pixels() {
        // 2x1: (B=30,G=20,R=10), (B=3,G=2,R=1)
        let buf = [30u8, 20, 10, 255, 3, 2, 1, 255];
        let image = bgra_to_rgb(&buf, 2, 1, 8).expect("convert");
        assert_eq!(image.get_pixel(0, 0).0, [10, 20, 30]);
        assert_eq!(image.get_pixel(1, 0).0, [1, 2, 3]);
    }
}
