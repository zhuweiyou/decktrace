"""加载牌面图片素材，并生成普通 / 底牌 / 反色三种展示状态。"""

from __future__ import annotations

from pathlib import Path
from typing import Dict, Tuple

from PIL import Image, ImageOps

from ...games.base import GameProfile

# 底牌标记色：介于“正色”和“反色”之间的蓝色调。
BOTTOM_TINT = (37, 99, 235)
BOTTOM_TINT_ALPHA = 0.42


class CardFaceFactory:
    def __init__(self, game: GameProfile) -> None:
        self.game = game
        self.asset_dir = game.card_assets_dir
        self._cache: Dict[Tuple[str, int, int, bool, bool], Image.Image] = {}
        self._source_cache: Dict[str, Image.Image] = {}

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
        cached = self._cache.get(key)
        if cached is not None:
            return cached

        image = self._load(card_id, width, height)
        if bottom:
            image = self._tint(image, BOTTOM_TINT, BOTTOM_TINT_ALPHA)
        elif invert:
            alpha = image.getchannel("A")
            image = ImageOps.invert(image.convert("RGB")).convert("RGBA")
            image.putalpha(alpha)
        self._cache[key] = image
        return image

    def _load(self, card_id: str, width: int, height: int) -> Image.Image:
        if self.asset_dir is None:
            raise FileNotFoundError(
                f"游戏 {self.game.id!r} 未配置牌面素材目录 card_assets_dir"
            )
        path = Path(self.asset_dir) / f"{card_id}.png"
        source = self._source_cache.get(card_id)
        if source is None:
            if not path.exists():
                raise FileNotFoundError(f"缺少牌面素材：{path}")
            source = Image.open(path).convert("RGBA")
            self._source_cache[card_id] = source
        return source.resize((width, height), Image.Resampling.NEAREST)

    @staticmethod
    def _tint(image: Image.Image, color: Tuple[int, int, int], alpha: float) -> Image.Image:
        alpha_channel = image.getchannel("A")
        overlay = Image.new("RGB", image.size, color)
        tinted = Image.blend(image.convert("RGB"), overlay, alpha)
        result = tinted.convert("RGBA")
        result.putalpha(alpha_channel)
        return result
