"""UI 抽象：TUI、GUI 或其他展示方式都实现 Renderer。"""

from __future__ import annotations

from abc import ABC, abstractmethod

from ...games.base import GameProfile
from ..deck import TrackerSnapshot


class Renderer(ABC):
    name: str = "unknown"

    @abstractmethod
    def start(self) -> None:
        raise NotImplementedError

    @abstractmethod
    def update(self, snapshot: TrackerSnapshot, game: GameProfile, status: str = "") -> None:
        """TUI 使用 status（fps/队列等）；纯展示 GUI 可以忽略。"""

    @abstractmethod
    def close(self) -> None:
        raise NotImplementedError

    @property
    @abstractmethod
    def is_open(self) -> bool:
        raise NotImplementedError
