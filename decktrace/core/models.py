"""跨平台、跨游戏共用的数据模型。"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Optional, Tuple

SUIT_SYMBOLS = {"H": "♥", "D": "♦", "S": "♠", "C": "♣"}


@dataclass(frozen=True)
class WindowInfo:
    """一个可被采集的窗口。macOS 用 CGWindowID，Windows 用 HWND。"""

    id: int
    title: str
    app: str
    pid: int = 0
    x: float = 0.0
    y: float = 0.0
    width: float = 0.0
    height: float = 0.0
    onscreen: bool = True
    layer: int = 0
    scale: float = 1.0

    @property
    def area(self) -> float:
        return self.width * self.height

    @property
    def size(self) -> str:
        return f"{self.width:.0f}x{self.height:.0f}"

    @property
    def position(self) -> str:
        return f"({self.x:.0f}, {self.y:.0f})"

    def __str__(self) -> str:
        return f"#{self.id} {self.app} / {self.title!r} {self.size} @ {self.position}"


@dataclass(frozen=True)
class WindowSelector:
    """不同游戏/不同平台的窗口特征。"""

    title: str = ""
    app: Optional[str] = None
    window_id: Optional[int] = None
    include_off_screen: bool = False
    min_width: float = 120
    min_height: float = 120


@dataclass(frozen=True)
class Detection:
    rank: str
    suit: str
    score: float
    box: Tuple[int, int, int, int]
    rank_score: float = 0.0
    suit_score: float = 0.0
    face_score: float = 0.0
    display: str = ""
    # 底牌栏里的牌：只是可见，不代表已打出，不能计入 seen。
    is_bottom: bool = False

    @property
    def card_id(self) -> str:
        return f"{self.rank}{self.suit}" if self.suit else self.rank

    @property
    def label(self) -> str:
        if self.display:
            return self.display
        if self.suit:
            return f"{self.rank}{SUIT_SYMBOLS.get(self.suit, self.suit)}"
        return self.rank

    def __str__(self) -> str:
        return f"{self.label}[{self.score:.2f}]"


@dataclass(frozen=True)
class RawFrame:
    """平台采集后的一帧，image 为 BGR ndarray。"""

    seq: int
    timestamp: float
    image: Any
    width: int = 0
    height: int = 0


@dataclass
class DetectionBatch:
    detections: list
    timestamp: float
    frame: Optional["Frame"] = None
    recognition_ms: float = 0.0
    seq: int = 0
    meta: dict = field(default_factory=dict)


@dataclass(frozen=True)
class Frame:
    """进入队列的一帧元数据。"""

    path: Optional[Path]
    captured_at: datetime
    window: WindowInfo
    width: int
    height: int
    scale: float
    elapsed: float

    @property
    def size(self) -> str:
        return f"{self.width}x{self.height}"

    @property
    def age(self) -> float:
        try:
            return max(
                (datetime.now().astimezone() - self.captured_at).total_seconds(), 0.0
            )
        except Exception:
            return 0.0

    def __str__(self) -> str:
        stamp = f"{self.captured_at:%H:%M:%S.%f}"[:-3]
        return f"{stamp}  {self.size}  scale={self.scale:.2f}  {self.elapsed * 1000:.0f}ms  {self.path}"
