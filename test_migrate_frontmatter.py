"""Tests for the migrate-frontmatter script.

Run with: python3 -m unittest test_migrate_frontmatter
The script has no `.py` extension, so it's loaded via importlib.
"""

import importlib.util
import tempfile
import unittest
from importlib.machinery import SourceFileLoader
from pathlib import Path

_path = str(Path(__file__).with_name("migrate-frontmatter"))
_loader = SourceFileLoader("migrate_frontmatter", _path)
_spec = importlib.util.spec_from_loader("migrate_frontmatter", _loader)
mf = importlib.util.module_from_spec(_spec)
_loader.exec_module(mf)


class ConvertLeaders(unittest.TestCase):
    def test_hash_leader_unchanged_behavior(self):
        src = "#!/usr/bin/env bash\n# Summary: list\n# Usage: p list\n\nset -e\n"
        out, changed = mf.convert(src)
        self.assertTrue(changed)
        self.assertIn("#@ summary: list", out)
        self.assertIn("#@ usage: p list", out)

    def test_double_slash_leader_emits_slash_marker(self):
        src = "#!/usr/bin/env node\n// Summary: a js cmd\n// Usage: p go\n\nrun();\n"
        out, changed = mf.convert(src)
        self.assertTrue(changed)
        self.assertIn("//@ summary: a js cmd", out)
        self.assertIn("//@ usage: p go", out)
        self.assertNotIn("#@", out)

    def test_dash_leader_emits_dash_marker(self):
        src = "-- Summary: a sql cmd\n-- Usage: p q\n\nSELECT 1;\n"
        out, _ = mf.convert(src)
        self.assertIn("--@ summary: a sql cmd", out)
        self.assertIn("--@ usage: p q", out)

    def test_repeated_semicolon_leader_lisp(self):
        src = ";;; Summary: a lisp cmd\n;;; Usage: p l\n\n(run)\n"
        out, _ = mf.convert(src)
        self.assertIn(";@ summary: a lisp cmd", out)
        self.assertIn(";@ usage: p l", out)


class ConvertEval(unittest.TestCase):
    def test_eval_added_when_flagged(self):
        src = "#!/usr/bin/env bash\n# Summary: cd somewhere\n\necho cd /tmp\n"
        out, changed = mf.convert(src, is_eval=True)
        self.assertTrue(changed)
        self.assertIn("#@ summary: cd somewhere", out)
        self.assertIn("#@ eval: true", out)

    def test_eval_only_no_old_frontmatter_preserves_body(self):
        src = "#!/usr/bin/env bash\n# a plain comment\n\necho cd /tmp\n"
        out, changed = mf.convert(src, is_eval=True)
        self.assertTrue(changed)
        self.assertIn("#@ eval: true", out)
        self.assertIn("# a plain comment", out)  # original comment kept
        self.assertIn("echo cd /tmp", out)

    def test_no_eval_when_not_flagged(self):
        src = "#!/usr/bin/env bash\n# Summary: x\n\necho hi\n"
        out, _ = mf.convert(src)
        self.assertNotIn("eval: true", out)


class RenameInPlace(unittest.TestCase):
    def test_in_place_renames_sh_file(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "myprog-sh-cd"
            src.write_text("#!/usr/bin/env bash\n# Summary: cd\n\necho cd /tmp\n")
            mf.main(["-i", str(src)])
            self.assertFalse(src.exists())
            dst = Path(d) / "myprog-cd"
            self.assertTrue(dst.exists())
            self.assertIn("#@ eval: true", dst.read_text())

    def test_in_place_does_not_rename_plain_file(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "myprog-who"
            src.write_text("#!/usr/bin/env bash\n# Summary: who\n\nwho\n")
            mf.main(["-i", str(src)])
            self.assertTrue(src.exists())
            self.assertNotIn("eval: true", src.read_text())

    def test_in_place_skips_rename_if_target_exists(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "myprog-sh-cd"
            src.write_text("#!/usr/bin/env bash\n# Summary: cd\n\necho cd /tmp\n")
            existing = Path(d) / "myprog-cd"
            existing.write_text("#!/bin/sh\n")
            mf.main(["-i", str(src)])
            # Source not clobbered away; target left intact.
            self.assertTrue(src.exists())
            self.assertEqual(existing.read_text(), "#!/bin/sh\n")


if __name__ == "__main__":
    unittest.main()
