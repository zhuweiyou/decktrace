"""Windows 采集后端预留。

TODO（Windows 实机实现时）：
1. 窗口可见时可用 Windows Graphics Capture / PrintWindow 获取窗口自身内容；
2. 或先用 Pillow ImageGrab 按窗口矩形抓屏作为 fallback；
3. 输出统一为 core.models.RawFrame（BGR ndarray + timestamp）。
"""

from __future__ import annotations

from pathlib import Path

from ...core.models import RawFrame, WindowInfo
from ..base import CaptureError, CaptureStream


class WindowsCaptureStream(CaptureStream):
    self_paced = False

    def __init__(self, window: WindowInfo, *, fps: float = 15.0) -> None:
        self.window = window
        self.target_fps = fps
        self.backend_name = "windows-capture-todo"

    def start(self) -> None:
        raise CaptureError("Windows 采集后端尚未实现（架构已预留）")

    def read(self, timeout: float = 2.0) -> RawFrame:
        raise CaptureError("Windows 采集后端尚未实现（架构已预留）")

    def stop(self) -> None:
        return
