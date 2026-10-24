"""Windows 平台装配。窗口枚举已提供，采集后端预留 TODO。"""

from __future__ import annotations

from pathlib import Path

from ...core.models import WindowInfo
from ..base import CaptureError, CaptureStream, Platform
from .capture import WindowsCaptureStream
from .window import WindowsWindowService


class WindowsPlatform(Platform):
    name = "windows"

    def __init__(self) -> None:
        self._windows = WindowsWindowService()

    @property
    def window_service(self) -> WindowsWindowService:
        return self._windows

    def create_capture_stream(
        self,
        window: WindowInfo,
        *,
        fps: float = 15.0,
        quality: float = 0.88,
        backend: str = "auto",
        out_dir: Path = Path("frames"),
    ) -> CaptureStream:
        raise CaptureError(
            "Windows 截图后端尚未实现。"
            "窗口枚举已可用；采集接口已抽象为 CaptureStream，"
            "后续在 platforms/windows/capture.py 实现即可。"
        )

    def capture_once(self, window: WindowInfo, out_path: Path, fmt: str = "png") -> Path:
        raise CaptureError("Windows 截图后端尚未实现（架构已预留）")
