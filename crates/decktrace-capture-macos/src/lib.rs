//! macOS ScreenCaptureKit 实现。窗口枚举与帧回调都在 Rust 进程内完成。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender, bounded};
use decktrace_capture::{CaptureError, CaptureStream, Platform, Result, WindowService};
use decktrace_core::{RawFrame, WindowInfo};
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
                scale: 1.0,
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
        let mut data = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for row in 0..height {
            let start = row as usize * bytes_per_row;
            let end = start + width as usize * 4;
            if end > slice.len() {
                break;
            }
            for chunk in slice[start..end].chunks_exact(4) {
                // BGRA -> RGB
                data.push(chunk[2]);
                data.push(chunk[1]);
                data.push(chunk[0]);
            }
        }
        if data.len() != (width as usize) * (height as usize) * 3 {
            return;
        }
        let Some(image) = image::RgbImage::from_raw(width, height, data) else {
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
    fps: f32,
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
            fps,
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

    fn target_fps(&self) -> f32 {
        self.fps
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

fn init_coregraphics() {
    unsafe extern "C" {
        fn sc_initialize_core_graphics();
    }
    unsafe { sc_initialize_core_graphics() }
}
