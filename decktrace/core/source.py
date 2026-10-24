"""识别来源抽象。

图片模板识别、内存扫描、其他来源都只需要产出 DetectionBatch；
去重、计数、表格和重置逻辑只消费 DetectionBatch，不关心来源。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Optional

from .models import DetectionBatch


@dataclass
class SourceStats:
    fps: float = 0.0
    recognition_ms: float = 0.0
    queue_size: int = 0
    dropped: int = 0
    errors: int = 0
    frame_age: float = 0.0
    backend: str = ""
    failed: bool = False


class DetectionSource(ABC):
    name: str = "unknown"

    @abstractmethod
    def start(self) -> None:
        raise NotImplementedError

    @abstractmethod
    def read(self, timeout: float = 2.0) -> Optional[DetectionBatch]:
        raise NotImplementedError

    @abstractmethod
    def stop(self) -> None:
        raise NotImplementedError

    @abstractmethod
    def stats(self) -> SourceStats:
        raise NotImplementedError
