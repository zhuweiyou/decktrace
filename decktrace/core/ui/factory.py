from __future__ import annotations

from ...games.base import GameProfile
from .base import Renderer
from .tui import TuiRenderer


def create_renderer(
    kind: str,
    game: GameProfile,
    *,
    color: bool = True,
    topmost: bool = True,
    highlight_bombs: bool = True,
) -> Renderer:
    if kind == "tui":
        return TuiRenderer(color=color)
    if kind == "gui":
        try:
            from .qt import QtCardRenderer
        except ImportError as exc:
            raise RuntimeError(
                "GUI 是默认 UI，需要 PySide6，请安装："
                ".venv/bin/python -m pip install -r requirements-gui.txt"
                "；调试时也可用 --ui tui"
            ) from exc
        return QtCardRenderer(
            game, topmost=topmost, highlight_bombs=highlight_bombs
        )
    raise ValueError(f"未知 UI：{kind}")
