import unittest

from decktrace.cli import build_parser


class UiDefaultTest(unittest.TestCase):
    def test_run_defaults_to_gui(self):
        args = build_parser().parse_args(["run"])
        self.assertEqual(args.ui, "gui")

    def test_tui_is_explicit_opt_in(self):
        args = build_parser().parse_args(["run", "--ui", "tui"])
        self.assertEqual(args.ui, "tui")


if __name__ == "__main__":
    unittest.main()
