"""内存读取识别来源（TODO）。

未来接内存扫描/读牌时，只要实现 DetectionSource：
- start(): 附加进程/读取快照；
- read(): 返回 DetectionBatch（detections + meta）；
- stop(): 释放句柄。

去重、计数、Rich 表格、重置框架不需要任何改动。
"""

from __future__ import annotations

from typing import Optional

from ..core.models import DetectionBatch
from ..core.source import DetectionSource, SourceStats
from ..games.base import GameProfile


class MemoryDetectionSource(DetectionSource):
    name = "memory"

    def __init__(self, game: GameProfile, process_name: str = "") -> None:
        self.game = game
        self.process_name = process_name

    def start(self) -> None:
        raise NotImplementedError(
            "内存识别来源尚未实现；请实现 platforms/games 对应的内存扫描适配器"
        )

    def read(self, timeout: float = 2.0) -> Optional[DetectionBatch]:
        return None

    def stop(self) -> None:
        return

    def stats(self) -> SourceStats:
        return SourceStats(backend="memory-todo")
