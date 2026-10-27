from __future__ import annotations

import sys
import unittest
from pathlib import Path

TESTDATA = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TESTDATA))

import generate_corpus
from corpus import registry
from corpus.spec import DEFAULT_TIER, TIERS


class RegistryTest(unittest.TestCase):
    def test_claims_cover_every_suffix_exactly_once(self):
        claimed = []
        for line in (TESTDATA / "claims.tsv").read_text(encoding="utf-8").splitlines():
            if line.startswith("#") or not line.strip():
                continue
            parts = line.split("\t")
            if len(parts) < 3:
                continue
            claimed.extend(e.strip().lstrip(".") for e in parts[2].split(";") if e.strip())
        self.assertEqual(len(registry.REGISTRY), 112)
        self.assertEqual(len(set(registry.SUFFIXES)), 112)
        self.assertEqual(set(claimed), set(registry.SUFFIXES))

    def test_only_selects_exact_suffixes(self):
        selected = registry.select("png,jpg,3fr")
        self.assertEqual([e.ext for e in selected], ["3fr", "jpg", "png"])

    def test_unknown_suffix_is_rejected(self):
        with self.assertRaises(ValueError):
            registry.select("png,not_a_format")

    def test_generation_exceptions_are_documented(self):
        doc = (TESTDATA / "GENERATION_EXCEPTIONS.md").read_text(encoding="utf-8")
        self.assertIn("crm", registry.GENERATION_EXCEPTIONS)
        self.assertIn("qtk", registry.GENERATION_EXCEPTIONS)
        for ext, reason in registry.GENERATION_EXCEPTIONS.items():
            self.assertIn(ext, registry.SUFFIXES)
            self.assertIn(f".{ext}", doc)
            self.assertTrue(reason.strip())


class CliTest(unittest.TestCase):
    def test_default_tier_is_10m(self):
        args = generate_corpus.parse_args([])
        self.assertEqual(args.tier, DEFAULT_TIER)
        self.assertEqual(DEFAULT_TIER, "10M")
        self.assertEqual(len(args.only_entries), 112)

    def test_default_out_is_temp_testdata(self):
        args = generate_corpus.parse_args([])
        self.assertEqual(args.out, (TESTDATA.parent.parent / ".temp" / "testdata").resolve())

    def test_all_is_explicit(self):
        args = generate_corpus.parse_args(["--tier", "all"])
        self.assertEqual(args.tier, "all")
        self.assertEqual(set(TIERS), {"1M", "10M", "100M"})
