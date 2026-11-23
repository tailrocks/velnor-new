# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import json
import unittest
from pathlib import Path

HERE = Path(__file__).parent
spec = importlib.util.spec_from_file_location("guard_tests", HERE / "test_private_names_guard.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
guard = base.guard
planted = base.planted
repository = base.repository
indexed = base.indexed

NAMES = ("zzplanted", "zzorg/zzrepo", "zz-two-parts")


def flagged(files, names=NAMES):
    tmp, root = indexed({path.encode(): text if isinstance(text, bytes) else text.encode() for path, text in files.items()})
    with tmp:
        return guard.scan(root, planted(*names))


class SpellingFixtureTests(unittest.TestCase):
    CASES = {
        "url": "see https://example.org/zzorg/other and https://github.com/zzplanted/x\n",
        "url_pair_with_git_suffix": "git clone https://github.com/zzorg/zzrepo.git\n",
        "ssh_pair_with_git_suffix": "git@github.com:ZzOrg/ZzRepo.GIT\n",
        "git_suffix_directory": "a/zzorg/zzrepo.git/info/refs\n",
        "at_prefix": "ping @ZzPlanted now\n",
        "percent_encoded_letter": "https://example.org/zz%70lanted\n",
        "percent_encoded_slash": "https://example.org/zzorg%2Fzzrepo\n",
        "percent_encoded_twice": "https://example.org/zz%2570lanted\n",
        "percent_encoded_uppercase_hex": "zz%50lanted and zz%2DTwo%2dParts\n",
        "fullwidth_letters": "see ｚｚｐｌａｎｔｅｄ here\n",
        "mathematical_letters": "see \U0001d433\U0001d433planted here\n",
        "zero_width_space": "zz​planted\n",
        "zero_width_joiner_and_bom": "zz‍﻿pl⁠anted\n",
        "soft_hyphen": "zzplan­ted\n",
        "combining_accent": "zzplantéd\n",
        "precomposed_accent": "zzplantéd\n",
        "json_unicode_escape": '"name": "\\u007a\\u007aplanted"\n',
        "json_surrogate_pair_escape": '"name": "\\ud835\\udc33\\ud835\\udc33planted"\n',
        "json_escaped_slash_pair": '{"repo": "zzorg\\/zzrepo"}\n',
        "json_escaped_slash_url": '{"url": "https:\\/\\/github.com\\/zzorg\\/zzrepo"}\n',
        "json_escaped_slash_with_unicode_escape": '{"repo": "zz\\u006frg\\/zzrepo"}\n',
        "html_entity": "zz&#112;lanted and zz&#x70;lanted\n",
        "owner_name_field": '{"ownerName": "zzplanted"}\n',
        "owner_name_snake_field": '{"owner_name": "zzplanted"}\n',
        "owner_url_value": '{"owner": "https://github.com/zzplanted/widget"}\n',
        "owner_at_prefixed_value": '{"account": "@zzplanted"}\n',
        "owner_spaced_value": '{"login": "z z p l a n t e d"}\n',
        "owner_non_ascii_value": '{"organization": "\\uff5a\\uff5aplanted"}\n',
        "owner_zero_width_value": '{"org": "zz\\u200bplanted"}\n',
        "owner_id_field": '{"owner_id": "zzplanted"}\n',
        "spaced_letters": "see z z p l a n t e d now\n",
        "spaced_letters_many_blanks": "z  z\tp l  a n t e d\n",
        "spaced_pair_letters": "z z o r g / z z r e p o\n",
        "spaced_around_slash": "zzorg / zzrepo\n",
        "nbsp_between_letters": "z\u00a0z\u00a0p\u00a0l\u00a0a\u00a0n\u00a0t\u00a0e\u00a0d\n",
        "title_case_spaced": "see Zz Two Parts here\n",
        "title_case_spaced_single_word": "see Zz Planted here\n",
        "hyphen_form": "see zz-two-parts here\n",
        "under_score_form": "see zz_two_parts here\n",
        "dot_form": "see zz.two.parts here\n",
        "no_separator_form": "see ZzTwoParts here\n",
        "mixed_separators": "see Zz two_parts here and zz - two . parts\n",
        "many_blanks_and_tabs": "Zz  Two\tParts\n",
        "nbsp_separated_words": "Zz\u00a0Two\u00a0Parts\n",
        "percent_encoded_space": "Zz%20Two%20Parts\n",
        "title_case_spaced_pair": "Zz Org / Zz Repo\n",
        "title_case_spaced_pair_without_blanks_around_slash": "Zz Org/Zz Repo\n",
        "spaced_word_after_prose": "the quick brown fox and Zz Planted jumped\n",
    }

    def test_every_spelling_is_flagged_in_any_file_type(self):
        for label, text in self.CASES.items():
            for path in ("a.md", "a.JSON", "noext"):
                with self.subTest(label=label, path=path):
                    self.assertEqual(len(flagged({path: text})), 1)

    def test_the_report_names_the_line_and_never_the_word(self):
        for label, text in self.CASES.items():
            with self.subTest(label=label):
                found = flagged({"a.md": "clean\n" + text})
                self.assertEqual(found, ["a.md:2"])

    def test_the_spellings_are_flagged_inside_a_path(self):
        for label, name in [
            ("percent", "docs/zz%70lanted.md"),
            ("fullwidth", "docs/ｚｚplanted.md"),
            ("zero_width", "docs/zz​planted.md"),
            ("spaced", "docs/z z p l a n t e d.md"),
            ("title_case_spaced", "docs/Zz Two Parts.md"),
            ("under_score", "docs/zz_two_parts.md"),
            ("dot_form", "docs/zz.two.parts.md"),
        ]:
            with self.subTest(label=label):
                tmp, root = indexed({name.encode(): b"plain\n"})
                with tmp:
                    found = guard.scan(root, planted(*NAMES))
                    self.assertEqual(len(found), 1)
                    self.assertTrue(found[0].startswith("ls-files line 1"))

    def test_clean_text_with_the_same_shapes_is_not_flagged(self):
        clean = "\n".join(
            [
                "https://example.org/zz%70lant and zzplant​",
                "ｚｚplant \U0001d433\U0001d433other",
                "zz&#112;lant zz two other",
                "zzorg / other and other/zzrepo and zz two other",
                "zz two, parts and zz | two | parts and zz (two) parts",
                "zzplan ten and zz two pars",
                "a b c d e f g h i j k l m",
                '{"ownerName": "example-org", "owner_id": "42"}',
                "https://github.com/example-org/example-repo-1.git",
                "café 中文 100% done 50%25 \\u00e9 &amp; &lt;",
            ]
        )
        self.assertEqual(flagged({"a.md": clean + "\n"}), [])

    def test_a_decoded_line_keeps_the_line_numbers_of_the_original(self):
        found = flagged({"a.md": "one\nzz%0a%0dplanted\n\ntwo\nzz%70lanted\n"})
        self.assertEqual(found, ["a.md:2", "a.md:5"])

    def test_decoding_a_huge_hostile_line_stays_linear(self):
        line = ("%" * 200_000 + "&" * 200_000 + "\\u" * 100_000 + "​" * 100_000).encode() + b"\n"
        self.assertEqual(flagged({"a.md": line}), [])

    def test_a_tracked_json_with_uppercase_extension_is_scanned_like_any_file(self):
        text = json.dumps({"ownerName": "ZZPLANTED", "owner_id": "zzplanted", "note": "ok"}, indent=2) + "\n"
        self.assertEqual(flagged({"docs/reviews/E.JSON": text}), ["docs/reviews/E.JSON:2", "docs/reviews/E.JSON:3"])


if __name__ == "__main__":
    unittest.main()
