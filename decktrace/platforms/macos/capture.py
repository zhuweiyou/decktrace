"""macOS 采集流：ScreenCaptureKit 常驻采集 + screencapture 回退。"""

from __future__ import annotations

import os
import time
from datetime import datetime
from pathlib import Path
from typing import Optional

import cv2
import numpy as np

from ...core.models import RawFrame, WindowInfo
from ..base import CaptureError, CaptureStream
from .legacy import capture_once as legacy_capture_once
from .legacy import capture_window
from .sck import JPEGFrame, SCKCapture, SCKError


def save_jpeg_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f"{path.stem}.{os.getpid()}.tmp{path.suffix}")
    tmp.write_bytes(data)
    os.replace(tmp, path)


class ScreenCaptureKitStream(CaptureStream):
    self_paced = True

    def __init__(
        self,
        window: WindowInfo,
        *,
        fps: float,
        quality: float,
        latest_path: Path,
    ) -> None:
        self.window = window
        self.target_fps = fps
        self.quality = quality
        self.latest_path = latest_path
        self.backend_name = "screen-capture-kit"
        self.sck = SCKCapture(window, fps=fps, quality=quality)
        self.seq = 0
        self.started_at = time.monotonic()
        self._first: Optional[JPEGFrame] = None
        self._last_save = 0.0

    def start(self) -> None:
        self.sck.start()
        self._first = self.sck.take_first()

    def _decode(self, raw: JPEGFrame) -> RawFrame:
        started = time.perf_counter()
        image = cv2.imdecode(np.frombuffer(raw.jpeg, np.uint8), cv2.IMREAD_COLOR)
        if image is None:
            raise CaptureError("无法解码 ScreenCaptureKit JPEG 帧")
        elapsed = time.perf_counter() - started
        self.seq += 1
        now = time.monotonic()
        if now - self._last_save >= 0.5:
            try:
                save_jpeg_atomic(self.latest_path, raw.jpeg)
            except OSError:
                pass
            self._last_save = now
        # elapsed 暂存到 RawFrame 无字段，由上层用 seq/timestamp 统计
        _ = elapsed
        return RawFrame(
            seq=self.seq,
            timestamp=raw.timestamp,
            image=image,
            width=image.shape[1],
            height=image.shape[0],
        )

    def read(self, timeout: float = 2.0) -> RawFrame:
        try:
            if self._first is not None:
                first, self._first = self._first, None
                return self._decode(first)
            return self._decode(self.sck.read(timeout=timeout))
        except SCKError as exc:
            raise CaptureError(str(exc)) from exc

    def stop(self) -> None:
        self.sck.stop()


class LegacyScreencaptureStream(CaptureStream):
    self_paced = False

    def __init__(
        self,
        window: WindowInfo,
        *,
        fps: float,
        latest_path: Path,
    ) -> None:
        self.window = window
        self.target_fps = fps
        self.latest_path = latest_path
        self.backend_name = "screencapture"
        self.seq = 0
        self.started_at = time.monotonic()

    def start(self) -> None:
        self.latest_path.parent.mkdir(parents=True, exist_ok=True)
        capture_window(self.window, self.latest_path, fmt="jpg")

    def read(self, timeout: float = 2.0) -> RawFrame:
        frame = capture_window(self.window, self.latest_path, fmt="jpg")
        image = cv2.imread(str(frame.path), cv2.IMREAD_COLOR)
        if image is None:
            raise CaptureError(f"无法读取截图：{frame.path}")
        self.seq += 1
        return RawFrame(
            seq=self.seq,
            timestamp=frame.captured_at.timestamp(),
            image=image,
            width=image.shape[1],
            height=image.shape[0],
        )

    def stop(self) -> None:
        return
