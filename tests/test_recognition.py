import unittest
from dataclasses import replace
from pathlib import Path

import cv2

from decktrace.core.deck import CardTracker
from decktrace.games import get_game
from decktrace.recognition import CardRecognizer

GAME = get_game("jj")

SHOTS = Path(__file__).resolve().parent.parent / "shots"


@unittest.skipUnless((SHOTS / "round_now.png").exists(), "缺少样本截图")
class RecognitionTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.recognizer = CardRecognizer(GAME)

    def test_sample_round(self):
        image = cv2.imread(str(SHOTS / "round_now.png"))
        result = self.recognizer.recognize(image)
        cards = {d.card_id for d in result.detections}
        expected = {
            "BJ", "2H", "2C", "AS", "AD", "KS", "JS", "10C", "10D",
            "9H", "9D", "7D", "6H", "5H", "5C", "3H", "3C",
            "6S", "6C", "5S", "5D", "4H", "4C",
            "10S", "AH", "8H",
        }
        self.assertEqual(cards, expected)
        # 积分/排名等纯数字不应出现
        self.assertNotIn("750", {d.rank for d in result.detections})

        # 顶部三张底牌只算“可见”，不应当成已出牌
        bottom = {d.card_id for d in result.detections if d.is_bottom}
        self.assertEqual(bottom, {"10S", "AH", "8H"})
        hand_card = next(d for d in result.detections if d.card_id == "2H")
        self.assertFalse(hand_card.is_bottom)

    def test_pre_bottom_detections_are_ignored(self):
        """真实截图上验证开局门控：三张底牌确认前，所有排除都无效。"""

        image = cv2.imread(str(SHOTS / "round_now.png"))
        detections = self.recognizer.recognize(image).detections
        bottom = [d for d in detections if d.is_bottom]
        self.assertEqual(len(bottom), 3)

        tracker = CardTracker(GAME, min_hits=1)
        # 翻牌动画阶段：底牌还没认全，手牌/动画牌都不入账
        tracker.observe([replace(d, is_bottom=False) for d in bottom])
        self.assertFalse(tracker.armed)
        self.assertEqual(tracker.seen_count, 0)

        # 第一次看全底牌：只记录，不立即开局
        tracker.observe(detections)
        self.assertFalse(tracker.armed)

        # 连续两帧稳定确认底牌后才开始记牌，并排除手牌
        tracker.observe(detections)
        self.assertTrue(tracker.armed)
        self.assertEqual(tracker.bottom_ids, {d.card_id for d in bottom})
        self.assertEqual(tracker.seen_count, len(detections) - 3)
        self.assertEqual(tracker.remaining, 54 - (len(detections) - 3))

    @unittest.skipUnless((SHOTS / "round_live.png").exists(), "缺少底牌背面样本")
    def test_face_down_bottom_does_not_start(self):
        """底牌还是背面时说明地主未决出，任何检测都不能开始记牌。"""

        image = cv2.imread(str(SHOTS / "round_live.png"))
        detections = self.recognizer.recognize(image).detections
        self.assertFalse(any(d.is_bottom for d in detections))

        tracker = CardTracker(GAME, min_hits=1)
        for _ in range(3):
            tracker.observe(detections)
        self.assertFalse(tracker.armed)
        self.assertEqual(tracker.seen_count, 0)

    @unittest.skipUnless((SHOTS / "bj_bottom_real.png").exists(), "缺少底牌大王样本")
    def test_bottom_jokers_flagged(self):
        image = cv2.imread(str(SHOTS / "bj_bottom_real.png"))
        bottom = {
            d.card_id for d in self.recognizer.recognize(image).detections if d.is_bottom
        }
        self.assertEqual(bottom, {"BJ", "JC", "JH"})

    @unittest.skipUnless((SHOTS / "joker_miss.png").exists(), "缺少小王样本")
    def test_compact_small_joker(self):
        image = cv2.imread(str(SHOTS / "joker_miss.png"))
        result = self.recognizer.recognize(image)
        jokers = [d for d in result.detections if d.rank in ("SJ", "BJ")]
        self.assertEqual([d.rank for d in jokers], ["SJ"])
        # 顶部底牌栏的小王是压缩绘制，必须用紧凑模板
        self.assertGreater(jokers[0].score, 0.9)


    @unittest.skipUnless((SHOTS / "joker_miss.png").exists(), "缺少小王样本")
    def test_compact_big_joker_color_branch(self):
        """底牌大王和小王共用紧凑字形，靠红/黑颜色区分。"""
        image = cv2.imread(str(SHOTS / "joker_miss.png"))
        small = next(
            d for d in self.recognizer.recognize(image).detections if d.rank == "SJ"
        )
        x, y, w, h = small.box
        red, dark = self.recognizer._masks(image)
        recolored = image.copy()
        patch = recolored[y:y + h, x:x + w]
        patch[dark[y:y + h, x:x + w] > 0] = (30, 30, 200)  # BGR 红
        jokers = [
            d
            for d in self.recognizer.recognize(recolored).detections
            if d.rank in ("SJ", "BJ")
        ]
        self.assertEqual([d.rank for d in jokers], ["BJ"])


FIXTURES = Path(__file__).resolve().parent / "fixtures"


@unittest.skipUnless((FIXTURES / "joker_strip.png").exists(), "缺少离线小王样本")
class JokerFixtureTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.recognizer = CardRecognizer(GAME)

    def test_small_joker_offline(self):
        image = cv2.imread(str(FIXTURES / "joker_strip.png"))
        jokers = [
            d
            for d in self.recognizer.recognize(image).detections
            if d.rank in ("SJ", "BJ")
        ]
        self.assertEqual([d.rank for d in jokers], ["SJ"])
        self.assertGreater(jokers[0].score, 0.9)

    def test_real_compact_big_joker(self):
        image = cv2.imread(str(FIXTURES / "joker_strip_bj.png"))
        jokers = [
            d
            for d in self.recognizer.recognize(image).detections
            if d.rank in ("SJ", "BJ")
        ]
        self.assertEqual([d.rank for d in jokers], ["BJ"])
        self.assertGreater(jokers[0].score, 0.8)

    def test_big_joker_color_branch_offline(self):
        image = cv2.imread(str(FIXTURES / "joker_strip.png"))
        small = next(
            d for d in self.recognizer.recognize(image).detections if d.rank == "SJ"
        )
        x, y, w, h = small.box
        _red, dark = self.recognizer._masks(image)
        recolored = image.copy()
        recolored[y:y + h, x:x + w][dark[y:y + h, x:x + w] > 0] = (30, 30, 200)
        jokers = [
            d
            for d in self.recognizer.recognize(recolored).detections
            if d.rank in ("SJ", "BJ")
        ]
        self.assertEqual([d.rank for d in jokers], ["BJ"])


if __name__ == "__main__":
    unittest.main()
