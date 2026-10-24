import unittest

from decktrace.core.deck import CardTracker
from decktrace.core.models import Detection
from decktrace.core.table import render_table
from decktrace.games import get_game

GAME = get_game("jj")


def det(rank, suit, score=0.95, bottom=False):
    return Detection(
        rank=rank,
        suit=suit,
        score=score,
        box=(0, 0, 10, 10),
        is_bottom=bottom,
    )


class TrackerTest(unittest.TestCase):
    def test_two_hits_before_commit(self):
        tracker = CardTracker(GAME, min_hits=2)
        self.assertEqual(tracker.observe([det("2", "H", 0.80)]), [])
        committed = tracker.observe([det("2", "H", 0.80)])
        self.assertEqual([d.card_id for d in committed], ["2H"])
        self.assertEqual(tracker.seen_count, 1)

    def test_duplicate_in_one_frame_counts_once(self):
        tracker = CardTracker(GAME, min_hits=2, instant_score=0.99)
        # 同一帧里底牌和手牌重复展示，不应凑成两次观察
        self.assertEqual(tracker.observe([det("9", "H", 0.80), det("9", "H", 0.80)]), [])
        committed = tracker.observe([det("9", "H", 0.80)])
        self.assertEqual([d.card_id for d in committed], ["9H"])

    def test_high_score_commits_immediately(self):
        tracker = CardTracker(GAME, min_hits=2, instant_score=0.92)
        committed = tracker.observe([det("K", "S", 0.99)])
        self.assertEqual([d.card_id for d in committed], ["KS"])

    def test_duplicate_display_does_not_double_count(self):
        tracker = CardTracker(GAME, min_hits=1)
        for _ in range(5):
            tracker.observe([det("A", "H"), det("A", "H")])
        self.assertEqual(tracker.seen_by_rank["A"], 1)
        self.assertEqual(tracker.counts()["A"], 3)

    def test_all_cards_of_rank(self):
        tracker = CardTracker(GAME, min_hits=1)
        tracker.observe([det("7", s) for s in ("H", "D", "S", "C")])
        self.assertEqual(tracker.counts()["7"], 0)
        self.assertEqual(tracker.remaining, 50)

    def test_bottom_cards_not_counted_until_played(self):
        tracker = CardTracker(GAME, min_hits=1)
        tracker.observe(
            [det("10", "S", bottom=True), det("A", "H", bottom=True), det("8", "H", bottom=True)]
        )
        self.assertEqual(tracker.seen_count, 0)
        self.assertEqual(tracker.remaining, 54)
        self.assertEqual(tracker.counts()["A"], 4)
        self.assertEqual(tracker.snapshot().bottom_ids, {"10S", "AH", "8H"})

    def test_bottom_card_removed_after_played(self):
        tracker = CardTracker(GAME, min_hits=1)
        tracker.observe([det("A", "H", bottom=True)])
        tracker.observe([det("A", "H")])
        self.assertEqual(tracker.seen_ids, {"AH"})
        self.assertEqual(tracker.bottom_ids, set())
        self.assertEqual(tracker.remaining, 53)

    def test_same_card_bottom_and_hand_commits_as_hand(self):
        tracker = CardTracker(GAME, min_hits=1)
        tracker.observe([det("A", "H", bottom=True), det("A", "H")])
        self.assertEqual(tracker.seen_ids, {"AH"})
        self.assertEqual(tracker.bottom_ids, set())

    def test_render_marks_bottom(self):
        tracker = CardTracker(GAME, min_hits=1)
        tracker.observe([det("A", "H", bottom=True)])
        text = render_table(tracker.snapshot(), GAME, color=False)
        self.assertIn("底牌（未出，仍计入剩余）", text)

    def test_reset(self):
        tracker = CardTracker(GAME, min_hits=1)
        tracker.observe([det("3", "H")])
        tracker.reset()
        self.assertEqual(tracker.seen_count, 0)
        self.assertEqual(tracker.counts()["3"], 4)

    def test_render_contains_all_ranks(self):
        tracker = CardTracker(GAME, min_hits=1)
        text = render_table(tracker.snapshot(), GAME, color=False)
        for rank in ("大王", "小王", "2", "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3"):
            self.assertIn(rank, text)


if __name__ == "__main__":
    unittest.main()


class AutoResetTest(unittest.TestCase):
    def setUp(self):
        import numpy as np
        from decktrace.core.runtime import SharedState

        self.state = SharedState(
            game=GAME, tracker=CardTracker(GAME, min_hits=1), auto_reset=True
        )
        self.image = np.zeros((1000, 1800, 3), dtype=np.uint8)

    def _hand(self, ranks):
        return [
            Detection(rank=rank, suit="H", score=0.9, box=(100 + i * 90, 800, 44, 50))
            for i, rank in enumerate(ranks)
        ]

    def test_new_hand_resets(self):
        self.state.tracker.observe(self._hand(["3", "4", "5"]))
        self.state._prev_hand_ids = {"3H", "4H", "5H"}
        self.state._prev_hand_ts = 0
        new_hand = self._hand(["6", "7", "8", "9", "10", "J", "Q", "K", "A", "2", "3", "4", "5"])
        # 手工放入 15 张已见牌，满足自动重置的 seen 条件
        for rank in ["3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A", "2"]:
            self.state.tracker.seen_ids.add(f"{rank}S")
        self.state._maybe_auto_reset(new_hand, 1000, 10.0)
        self.assertEqual(self.state.auto_reset_count, 1)
        self.assertEqual(self.state.tracker.seen_count, 0)

    def test_playing_cards_does_not_reset(self):
        old = self._hand(["3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A", "2"])
        self.state._prev_hand_ids = {d.card_id for d in old}
        self.state._prev_hand_ts = 0
        for rank in ["3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A", "2"]:
            self.state.tracker.seen_ids.add(f"{rank}S")
        # 出牌后手牌是旧手牌的子集
        self.state._maybe_auto_reset(old[:-2], 1000, 10.0)
        self.assertEqual(self.state.auto_reset_count, 0)
