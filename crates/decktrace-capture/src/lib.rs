//! 平台采集实现：macOS 与 Windows 通过条件编译选择。

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use decktrace_core::{CaptureError, Platform};

#[cfg(target_os = "macos")]
pub use macos::MacPlatform as PlatformImpl;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform as PlatformImpl;

/// 创建当前平台的采集实现。
pub fn create_platform() -> Result<Box<dyn Platform>, CaptureError> {
    Ok(Box::new(PlatformImpl::new()?))
}
