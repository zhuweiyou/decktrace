"""macOS 平台装配。"""

from __future__ import annotations

from pathlib import Path
from typing import Optional

from ...core.models import WindowInfo
from ..base import CaptureStream, Platform
from .capture import LegacyScreencaptureStream, ScreenCaptureKitStream
from .legacy import capture_once as legacy_capture_once
from .window import MacOSWindowService


class MacOSPlatform(Platform):
    name = "macos"

    def __init__(self) -> None:
        self._windows = MacOSWindowService()
        self.last_fallback: Optional[str] = None

    @property
    def window_service(self) -> MacOSWindowService:
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
        latest = out_dir / "latest.jpg"
        if backend == "legacy":
            return LegacyScreencaptureStream(window, fps=fps, latest_path=latest)
        if backend not in ("auto", "sck"):
            raise ValueError(f"未知采集后端：{backend}")

        stream = ScreenCaptureKitStream(
            window, fps=fps, quality=quality, latest_path=latest
        )
        try:
            stream.start()
            self.last_fallback = None
            return stream
        except Exception as exc:
            if backend == "sck":
                raise
            self.last_fallback = str(exc)
            return LegacyScreencaptureStream(
                window, fps=fps, latest_path=latest
            )

    def capture_once(self, window: WindowInfo, out_path: Path, fmt: str = "png") -> Path:
        frame = legacy_capture_once(window, out_path, fmt=fmt)
        return frame.path
