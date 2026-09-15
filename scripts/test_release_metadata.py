import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "release_metadata", Path(__file__).with_name("release-metadata.py")
)
metadata = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metadata)


class ReleasePolicyTests(unittest.TestCase):
    def test_stable_release_updates_version_minor_and_latest(self):
        self.assertEqual(
            metadata.release_policy("refs/tags/v0.1.0"),
            (["0.1.0", "0.1", "latest"], False),
        )

    def test_prereleases_never_update_stable_aliases(self):
        for version in ("0.1.0-rc.1", "2.4.0-beta.2", "1.0.0-alpha", "1.0.0-0"):
            with self.subTest(version=version):
                self.assertEqual(
                    metadata.release_policy(f"refs/tags/v{version}"), ([version], True)
                )

    def test_branches_keep_their_own_alias(self):
        for branch in ("main", "develop"):
            self.assertEqual(
                metadata.release_policy(f"refs/heads/{branch}"), ([branch], False)
            )

    def test_invalid_or_unsupported_refs_fail_before_publication(self):
        for ref in (
            "refs/heads/feature", "refs/pull/1/merge", "refs/tags/v1.0",
            "refs/tags/v01.0.0", "refs/tags/v1.0.0-rc.01", "refs/tags/v1.0.0-",
            "refs/tags/v1.0.0+build.1", "refs/tags/v1.0.0-rc,latest",
        ):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                metadata.release_policy(ref)


if __name__ == "__main__":
    unittest.main()
