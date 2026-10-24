"""识别来源工厂。

image: 平台采集流 + 模板识别
memory: 预留，未来内存扫描
"""

from __future__ import annotations

from pathlib import Path
from typing import Optional

from ..core.models import WindowInfo
from ..core.source import DetectionSource
from ..games.base import GameProfile
from ..platforms.base import Platform, PlatformError
from ..recognition import CardRecognizer
from .image import ImageDetectionSource
from .memory import MemoryDetectionSource


def create_detection_source(
    kind: str,
    *,
    game: GameProfile,
    platform: Platform,
    window: Optional[WindowInfo],
    out_dir: Path,
    fps: float = 15.0,
    quality: float = 0.88,
    capture_backend: str = "auto",
    debug: bool = False,
) -> DetectionSource:
    if kind == "memory":
        raise PlatformError(
            "memory 来源尚未实现；架构已预留 MemoryDetectionSource。"
            f"当前 {game.name} 支持：{', '.join(game.supported_sources)}"
        )
    if kind != "image":
        raise PlatformError(f"未知识别来源：{kind}")
    if window is None:
        raise PlatformError("image 来源需要窗口")
    recognizer = CardRecognizer(game)
    stream = platform.create_capture_stream(
        window,
        fps=fps,
        quality=quality,
        backend=capture_backend,
        out_dir=out_dir,
    )
    return ImageDetectionSource(
        stream,
        recognizer,
        window,
        fps=fps,
        debug_dir=(out_dir if debug else None),
    )
