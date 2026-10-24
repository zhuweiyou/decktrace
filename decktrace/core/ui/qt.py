"""Qt GUI：一张牌一张图片，横向重叠成一行，窗口置顶，不显示 fps。"""

from __future__ import annotations

from typing import List, Optional, Tuple

from ... import APP_NAME
from ...games.base import GameProfile
from ..deck import TrackerSnapshot
from .base import Renderer
from .cards import CardFaceFactory
from .highlight import bomb_card_ids, card_visual_state


class QtCardRenderer(Renderer):
    name = "gui"

    def __init__(
        self, game: GameProfile, *, topmost: bool = True, highlight_bombs: bool = True
    ) -> None:
        self.game = game
        self.topmost = topmost
        self.highlight_bombs = highlight_bombs
        self.app = None
        self.window = None
        self._factory = CardFaceFactory(game)
        self._pixmaps: List[object] = []
        self._cards: List[Tuple[int, object]] = []
        self._last_key: Optional[Tuple[Tuple[str, ...], frozenset]] = None
        self._size = (0, 0)
        self._placed = False
        self._open = False

    def start(self) -> None:
        from PySide6.QtCore import Qt
        from PySide6.QtWidgets import QApplication

        self.app = QApplication.instance() or QApplication([])
        self.app.setQuitOnLastWindowClosed(False)
        self.window = _CardWindow(self)
        if self.topmost:
            self.window.setWindowFlags(
                self.window.windowFlags() | Qt.WindowStaysOnTopHint
            )
        self.window.setWindowTitle(APP_NAME)
        self.window.show()
        self.window.raise_()
        self.window.activateWindow()
        self._open = True
        self.app.processEvents()

    def update(self, snapshot: TrackerSnapshot, game: GameProfile, status: str = "") -> None:
        if not self._open or self.window is None:
            return
        remaining = game.remaining_card_ids(snapshot.seen_ids)
        bottom_ids = frozenset(snapshot.bottom_ids)
        key = (remaining, bottom_ids)
        if key != self._last_key:
            self._last_key = key
            self._rebuild(remaining, bottom_ids)
        self.app.processEvents()

    def _rebuild(
        self, remaining: Tuple[str, ...], bottom_ids: frozenset
    ) -> None:
        from PIL.ImageQt import ImageQt
        from PySide6.QtGui import QPixmap

        # 用户素材固定 15x30，不缩放。窗口固定能放下 54 张。
        card_w, card_h = 15, 30
        margin = 4
        width = card_w * 54 + margin * 2
        height = card_h + margin * 2
        self._pixmaps = []
        self._cards = []
        bombs = bomb_card_ids(remaining, self.game) if self.highlight_bombs else set()
        for index, card_id in enumerate(remaining):
            state = card_visual_state(card_id, bottom_ids, bombs)
            pil = self._factory.image(
                card_id,
                card_w,
                card_h,
                invert=state == "bomb",
                bottom=state == "bottom",
            )
            qimg = ImageQt(pil.convert("RGBA"))
            pix = QPixmap.fromImage(qimg)
            self._pixmaps.append(pix)
            self._cards.append((margin + index * card_w, pix))
        if self._size != (width, height):
            self._size = (width, height)
            self.window.resize(width, height)
        self._place()
        self.window.update()

    def _place(self) -> None:
        # 只定位一次；用户拖动后不要每次牌数变化又拉回中间
        if self._placed:
            return
        from PySide6.QtGui import QGuiApplication

        screen = QGuiApplication.primaryScreen().availableGeometry()
        width, height = self._size
        x = screen.x() + max(0, (screen.width() - width) // 2)
        y = screen.y() + 40
        self.window.move(x, y)
        self._placed = True

    def close(self) -> None:
        if self.window is not None:
            try:
                self.window.close()
                self.window.deleteLater()
            except Exception:
                pass
        self.window = None
        self._open = False

    @property
    def is_open(self) -> bool:
        return self._open


class _CardWindow:
    """延迟导入 Qt 的轻量窗口，避免 TUI 模式依赖 PySide6。"""

    def __new__(cls, renderer: QtCardRenderer):
        from PySide6.QtCore import Qt
        from PySide6.QtGui import QColor, QPainter
        from PySide6.QtWidgets import QWidget

        class CardWidget(QWidget):
            def __init__(self):
                super().__init__()
                self.renderer = renderer
                self.setStyleSheet("background:#f1ece1;")

            def paintEvent(self, event):
                painter = QPainter(self)
                painter.fillRect(self.rect(), QColor("#f1ece1"))
                if not self.renderer._cards:
                    painter.setPen(QColor("#6b7280"))
                    font = painter.font()
                    font.setPointSize(18)
                    font.setBold(True)
                    painter.setFont(font)
                    painter.drawText(self.rect(), Qt.AlignCenter, "牌已出完")
                    return
                for x, pix in self.renderer._cards:
                    painter.drawPixmap(x, 4, pix)

            def closeEvent(self, event):
                self.renderer._open = False
                event.accept()

        return CardWidget()
