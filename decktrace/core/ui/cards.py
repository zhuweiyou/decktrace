"""把牌 ID 绘制成可展示的牌面图片。"""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path
from typing import Dict, Tuple

from PIL import Image, ImageDraw, ImageFont, ImageOps

from ...games.base import GameProfile

# 底牌标记色：介于“正色”和“反色”之间的蓝色调。
BOTTOM_TINT = (37, 99, 235)
BOTTOM_TINT_ALPHA = 0.42

FONT_CANDIDATES = (
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "C:/Windows/Fonts/arialuni.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
)


def _find_font() -> str:
    for path in FONT_CANDIDATES:
        if Path(path).exists():
            return path
    return ""


class CardFaceFactory:
    def __init__(self, game: GameProfile) -> None:
        self.game = game
        self.font_path = _find_font()
        self.asset_dir = game.card_assets_dir
        self._cache: Dict[Tuple[str, int, int, bool, bool], Image.Image] = {}
        self._source_cache: Dict[str, Image.Image] = {}

    def _font(self, size: int):
        if self.font_path:
            try:
                return ImageFont.truetype(self.font_path, size)
            except OSError:
                pass
        return ImageFont.load_default()

    def image(
        self,
        card_id: str,
        width: int,
        height: int,
        *,
        invert: bool = False,
        bottom: bool = False,
    ) -> Image.Image:
        key = (card_id, width, height, invert, bottom)
        if key in self._cache:
            return self._cache[key]

        image = None
        if self.asset_dir is not None:
            path = Path(self.asset_dir) / f"{card_id}.png"
            if path.exists():
                source = self._source_cache.get(card_id)
                if source is None:
                    source = Image.open(path).convert("RGBA")
                    self._source_cache[card_id] = source
                image = source.resize((width, height), Image.Resampling.NEAREST)

        if image is None:
            image = self._render(card_id, width, height)
        if bottom:
            image = self._tint(image, BOTTOM_TINT, BOTTOM_TINT_ALPHA)
        elif invert:
            alpha = image.getchannel("A")
            image = ImageOps.invert(image.convert("RGB")).convert("RGBA")
            image.putalpha(alpha)
        self._cache[key] = image
        return image

    @staticmethod
    def _tint(image: Image.Image, color: Tuple[int, int, int], alpha: float) -> Image.Image:
        alpha_channel = image.getchannel("A")
        overlay = Image.new("RGB", image.size, color)
        tinted = Image.blend(image.convert("RGB"), overlay, alpha)
        result = tinted.convert("RGBA")
        result.putalpha(alpha_channel)
        return result

    def _render(self, card_id: str, width: int, height: int) -> Image.Image:
        image = Image.new("RGBA", (width, height), (0, 0, 0, 0))
        draw = ImageDraw.Draw(image)
        radius = max(3, width // 12)
        border = max(1, width // 40)
        draw.rounded_rectangle(
            (0, 0, width - 1, height - 1),
            radius=radius,
            fill=(251, 247, 237, 255),
            outline=(150, 139, 121, 255),
            width=border,
        )

        if card_id in self.game.labels:
            big = card_id == "BJ"
            color = (190, 30, 45) if big else (35, 35, 35)
            mark = "大" if big else "小"
            rank_font = self._font(max(10, int(height * 0.20)))
            king_font = self._font(max(14, int(height * 0.42)))
            draw.text(
                (width * 0.08, height * 0.02),
                mark,
                font=rank_font,
                fill=color,
                anchor="la",
            )
            draw.text(
                (width * 0.50, height * 0.56),
                "王",
                font=king_font,
                fill=color,
                anchor="mm",
            )
            return image

        suit = card_id[-1]
        rank = card_id[:-1]
        color = (190, 30, 45) if suit in ("H", "D") else (30, 30, 30)
        symbol = {"H": "♥", "D": "♦", "S": "♠", "C": "♣"}.get(suit, suit)
        rank_font = self._font(max(10, int(height * 0.36)))
        suit_font = self._font(max(8, int(height * 0.22)))
        center_font = self._font(max(12, int(height * 0.42)))
        draw.text(
            (width * 0.09, height * 0.02),
            rank,
            font=rank_font,
            fill=color,
            anchor="la",
        )
        draw.text(
            (width * 0.09, height * 0.31),
            symbol,
            font=suit_font,
            fill=color,
            anchor="la",
        )
        if width >= 34:
            draw.text(
                (width * 0.62, height * 0.62),
                symbol,
                font=center_font,
                fill=color,
                anchor="mm",
            )
        return image
