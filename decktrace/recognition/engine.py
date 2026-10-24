"""通用牌面识别引擎：点数 + 花色成对校验。

游戏差异（模板目录、牌面高度、大小王区域、阈值）由 GameProfile 提供，
业务层不出现具体游戏常量。
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple

import cv2
import numpy as np

from ..core.models import Detection
from ..games.base import GameProfile


@dataclass
class RecognitionResult:
    detections: List[Detection]
    elapsed_ms: float
    frame_width: int
    frame_height: int


class CardRecognizer:
    def __init__(self, game: GameProfile) -> None:
        self.game = game
        self.cfg = game.recognition
        base = Path(game.templates_dir)
        rank_dir = base / "ranks"
        suit_dir = base / "suits"
        if not rank_dir.exists() or not suit_dir.exists():
            raise FileNotFoundError(
                f"缺少模板目录：{rank_dir} / {suit_dir}，"
                f"请先运行 {game.id} 的模板构建脚本"
            )

        self.ranks: Dict[str, np.ndarray] = {
            path.stem: cv2.imread(str(path), cv2.IMREAD_GRAYSCALE)
            for path in sorted(rank_dir.glob("*.png"))
        }
        self.suits: Dict[str, np.ndarray] = {
            path.stem: cv2.imread(str(path), cv2.IMREAD_GRAYSCALE)
            for path in sorted(suit_dir.glob("*.png"))
        }
        self.joker_templates: Dict[str, np.ndarray] = {}
        for name in {self.cfg.joker_template, self.cfg.joker_compact_template}:
            if not name:
                continue
            template = cv2.imread(str(base / name), cv2.IMREAD_GRAYSCALE)
            if template is not None:
                self.joker_templates[name] = template
        if not self.ranks or len(self.suits) != 4:
            raise FileNotFoundError("点数或花色模板不完整")

    # ---------- 基础图像处理 ----------

    @staticmethod
    def _masks(image: np.ndarray) -> Tuple[np.ndarray, np.ndarray]:
        b, g, r = cv2.split(image)
        red = (
            (r.astype(int) > 110)
            & (r.astype(int) - g.astype(int) > 45)
            & (r.astype(int) - b.astype(int) > 35)
        ).astype(np.uint8) * 255
        dark = ((r < 90) & (g < 90) & (b < 90)).astype(np.uint8) * 255
        return red, dark

    @staticmethod
    def _components(mask: np.ndarray):
        n, _labels, stats, _centroids = cv2.connectedComponentsWithStats(mask, 8)
        return [
            (int(x), int(y), int(w), int(h), int(area))
            for x, y, w, h, area in stats[1:]
        ]

    def _normalize_rank(self, crop: np.ndarray) -> np.ndarray:
        h, w = crop.shape[:2]
        nh = self.cfg.rank_template_height
        nw = max(1, round(w * nh / h))
        resized = cv2.resize(crop, (nw, nh), interpolation=cv2.INTER_AREA)
        _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
        return resized

    @staticmethod
    def _normalize_suit(crop: np.ndarray) -> np.ndarray:
        size = 40
        h, w = crop.shape[:2]
        scale = (size - 6) / max(h, w)
        rw, rh = max(1, round(w * scale)), max(1, round(h * scale))
        resized = cv2.resize(crop, (rw, rh), interpolation=cv2.INTER_AREA)
        _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
        canvas = np.zeros((size, size), np.uint8)
        x = (size - rw) // 2
        y = (size - rh) // 2
        canvas[y:y + rh, x:x + rw] = resized
        return canvas

    # ---------- 定位 ----------

    @staticmethod
    def _rank_bbox(
        mask: np.ndarray, sx: int, sy: int, sw: int, sh: int
    ) -> Optional[Tuple[int, int, int, int]]:
        y0 = max(0, sy - int(2.2 * sh))
        y1 = max(1, sy - 2)
        x0 = max(0, sx - int(0.2 * sw))
        x1 = min(mask.shape[1], sx + int(1.45 * sw))
        sub = mask[y0:y1, x0:x1]
        glyphs = []
        for x, y, w, h, area in CardRecognizer._components(sub):
            if h < max(8, int(0.2 * sh)) or h > int(2.0 * sh):
                continue
            if w > int(1.8 * sh) or area < max(20, int(0.01 * sh * sh)):
                continue
            if y == 0 and w > int(0.8 * sh):
                continue
            glyphs.append((x, y, w, h))
        if not glyphs:
            return None
        x = min(glyph[0] for glyph in glyphs)
        y = min(glyph[1] for glyph in glyphs)
        x2 = max(glyph[0] + glyph[2] for glyph in glyphs)
        y2 = max(glyph[1] + glyph[3] for glyph in glyphs)
        return x0 + x, y0 + y, x2 - x, y2 - y

    @staticmethod
    def _face_fraction(image: np.ndarray, x: int, y: int, w: int, h: int) -> float:
        pad = 8
        x0 = max(0, x - pad)
        y0 = max(0, y - pad)
        x1 = min(image.shape[1], x + w + pad)
        y1 = min(image.shape[0], y + h + pad)
        patch = image[y0:y1, x0:x1]
        if patch.size == 0:
            return 0.0
        b, g, r = cv2.split(patch)
        mx = np.maximum(np.maximum(r, g), b)
        mn = np.minimum(np.minimum(r, g), b)
        okay = (mx > 150) & ((mx.astype(int) - mn.astype(int)) < 35)
        return float(okay.mean())

    # ---------- 分类 ----------

    def _classify_rank(self, crop: np.ndarray) -> Tuple[str, float, float]:
        if crop.size == 0:
            return "", 0.0, 0.0
        normalized = self._normalize_rank(crop)
        scores: List[Tuple[str, float]] = []
        for name, template in self.ranks.items():
            resized = cv2.resize(
                normalized,
                (template.shape[1], template.shape[0]),
                interpolation=cv2.INTER_AREA,
            )
            _n, resized = cv2.threshold(resized, 96, 255, cv2.THRESH_BINARY)
            a = resized > 0
            b = template > 0
            iou = (a & b).sum() / max((a | b).sum(), 1)
            scores.append((name, float(iou)))
        scores.sort(key=lambda item: item[1], reverse=True)
        best_name, best_score = scores[0]
        second_score = scores[1][1] if len(scores) > 1 else 0.0
        return best_name, best_score, second_score

    def _classify_suit(self, crop: np.ndarray, color: str) -> Tuple[str, float]:
        if crop.size == 0:
            return "", 0.0
        normalized = self._normalize_suit(crop)
        options = ("H", "D") if color == "red" else ("S", "C")
        best_name, best_score = "", 0.0
        for name in options:
            template = self.suits[name]
            a = normalized > 0
            b = template > 0
            iou = (a & b).sum() / max((a | b).sum(), 1)
            if iou > best_score:
                best_name, best_score = name, float(iou)
        return best_name, best_score

    # ---------- 大小王 ----------

    def _detect_jokers(
        self, image: np.ndarray, combined: np.ndarray, red: np.ndarray, dark: np.ndarray
    ) -> List[Detection]:
        detections: List[Detection] = []
        if not self.joker_templates:
            return detections

        height, width = combined.shape
        base_scale = width / max(1, self.cfg.reference_width)
        for band in self.cfg.joker_bands:
            source = self.joker_templates.get(band.template)
            if source is None:
                continue
            y0 = int(height * band.top_ratio)
            y1 = min(height, int(height * band.bottom_ratio))
            if y1 <= y0 + 10:
                continue
            search = combined[y0:y1, :]
            base_h = source.shape[0]
            for prior in band.scales:
                scale = prior * base_scale
                template_h = max(12, round(base_h * scale))
                template_w = max(6, round(source.shape[1] * template_h / base_h))
                if template_h >= search.shape[0] or template_w >= search.shape[1]:
                    continue
                template = cv2.resize(
                    source, (template_w, template_h), interpolation=cv2.INTER_AREA
                )
                _n, template = cv2.threshold(template, 96, 255, cv2.THRESH_BINARY)
                result = cv2.matchTemplate(search, template, cv2.TM_CCOEFF_NORMED)
                ys, xs = np.where(result >= 0.62)
                for yy, xx in zip(ys, xs):
                    score = float(result[yy, xx])
                    x = int(xx)
                    y = int(yy) + y0
                    red_count = int(
                        (red[y:y + template_h, x:x + template_w] > 0).sum()
                    )
                    dark_count = int(
                        (dark[y:y + template_h, x:x + template_w] > 0).sum()
                    )
                    if max(red_count, dark_count) < 50:
                        continue
                    rank = "BJ" if red_count > dark_count else "SJ"
                    face = self._face_fraction(image, x, y, template_w, template_h)
                    if face < self.cfg.face_min_fraction:
                        continue
                    detections.append(
                        Detection(
                            rank=rank,
                            suit="",
                            score=score,
                            box=(x, y, template_w, template_h),
                            rank_score=score,
                            suit_score=1.0,
                            face_score=face,
                            display=self.game.label(rank),
                        )
                    )

        detections.sort(key=lambda d: d.score, reverse=True)
        kept: List[Detection] = []
        for det in detections:
            x, y, w, h = det.box
            cx, cy = x + w / 2, y + h / 2
            if any(
                abs(cx - (k.box[0] + k.box[2] / 2)) < max(w, k.box[2]) * 0.35
                and abs(cy - (k.box[1] + k.box[3] / 2)) < max(h, k.box[3]) * 0.25
                for k in kept
            ):
                continue
            kept.append(det)
        return kept

    # ---------- 对外接口 ----------

    def recognize(
        self, image: np.ndarray, *, detect_jokers: bool = True
    ) -> RecognitionResult:
        import time

        started = time.perf_counter()
        red, dark = self._masks(image)
        combined = cv2.bitwise_or(red, dark)
        raw: List[Detection] = []

        for color, mask in (("red", red), ("black", dark)):
            for sx, sy, sw, sh, area in self._components(mask):
                if not (12 <= sw <= 80 and 12 <= sh <= 85):
                    continue
                if not (0.65 <= sw / sh <= 1.45):
                    continue
                if area / (sw * sh) < 0.28 or area < 120:
                    continue

                rank_box = self._rank_bbox(mask, sx, sy, sw, sh)
                if rank_box is None:
                    continue
                rx, ry, rw, rh = rank_box
                rank_crop = mask[ry:ry + rh, rx:rx + rw]
                rank, rank_score, second_score = self._classify_rank(rank_crop)
                suit, suit_score = self._classify_suit(
                    mask[sy:sy + sh, sx:sx + sw], color
                )
                if rank_score < self.cfg.rank_min_score:
                    continue
                if suit_score < self.cfg.suit_min_score:
                    continue
                if rank_score - second_score < self.cfg.rank_margin:
                    continue

                face = self._face_fraction(image, sx, sy, sw, sh)
                if face < self.cfg.face_min_fraction:
                    continue

                score = rank_score * 0.7 + suit_score * 0.3
                raw.append(
                    Detection(
                        rank=rank,
                        suit=suit,
                        score=float(score),
                        box=(sx, sy, sw, sh),
                        rank_score=rank_score,
                        suit_score=suit_score,
                        face_score=face,
                        display=self.game.label(rank, suit),
                    )
                )

        raw.sort(key=lambda d: d.score, reverse=True)
        kept: List[Detection] = []
        for det in raw:
            x, y, w, h = det.box
            cx, cy = x + w / 2, y + h / 2
            if any(
                abs(cx - (k.box[0] + k.box[2] / 2)) < min(w, k.box[2]) * 0.6
                and abs(cy - (k.box[1] + k.box[3] / 2)) < min(h, k.box[3]) * 0.6
                for k in kept
            ):
                continue
            kept.append(det)

        if detect_jokers:
            kept.extend(self._detect_jokers(image, combined, red, dark))

        # 底牌识别完全交给游戏 profile；通用引擎不假设位置和方式。
        if self.game.bottom is not None:
            kept = self.game.bottom.mark(image, kept)

        elapsed_ms = (time.perf_counter() - started) * 1000
        return RecognitionResult(
            detections=sorted(kept, key=lambda d: (d.box[1] // 100, d.box[0])),
            elapsed_ms=elapsed_ms,
            frame_width=image.shape[1],
            frame_height=image.shape[0],
        )


def annotate(image: np.ndarray, detections: Sequence[Detection]) -> np.ndarray:
    out = image.copy()
    for det in detections:
        x, y, w, h = det.box
        cv2.rectangle(out, (x, y), (x + w, y + h), (0, 255, 0), 2)
        cv2.putText(
            out,
            det.label,
            (x, max(16, y - 5)),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.65,
            (0, 255, 255),
            2,
        )
    return out
