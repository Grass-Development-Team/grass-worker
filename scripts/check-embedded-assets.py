#!/usr/bin/env python3
"""Exercise the real asset crate across incremental Cargo builds and profiles."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="grass-assets-check-") as temporary:
        fixture = Path(temporary)
        assets = fixture / "crates/assets"
        (assets / "src").mkdir(parents=True)
        for name in ("Cargo.toml", "build.rs", "src/lib.rs"):
            shutil.copy2(ROOT / "crates/assets" / name, assets / name)
        manifest = re.sub(
            r"members = \[.*?\n\]",
            'members = ["crates/assets", "probe"]',
            (ROOT / "Cargo.toml").read_text(),
            count=1,
            flags=re.S,
        )
        (fixture / "Cargo.toml").write_text(manifest)
        shutil.copy2(ROOT / "Cargo.lock", fixture / "Cargo.lock")
        probe = fixture / "probe"
        (probe / "src").mkdir(parents=True)
        (probe / "Cargo.toml").write_text(
            '[package]\nname = "embedded-assets-probe"\nversion = "0.0.0"\n'
            'edition.workspace = true\n[dependencies]\ngrass-assets.workspace = true\n'
        )
        (probe / "src/main.rs").write_text(
            'fn main() {\n'
            '    let key = std::env::args().nth(1).unwrap();\n'
            '    match grass_assets::get(&key) {\n'
            '        Some(file) => print!("{}", std::str::from_utf8(&file.data).unwrap()),\n'
            '        None => print!("<missing>"),\n'
            '    }\n}\n'
        )
        dist = fixture / "apps/console/dist"
        dist.mkdir(parents=True)
        (dist / "index.html").write_text("console-build-A")
        (dist / "old.txt").write_text("old-asset")
        environment = os.environ.copy()
        target = Path(environment.get("CARGO_TARGET_DIR", ROOT / "target"))
        environment["CARGO_TARGET_DIR"] = str(target.resolve() / "assets-regression")
        initialized = False

        def run(release=True, key="index.html", failure=False):
            nonlocal initialized
            command = ["cargo", "run", "--quiet", "-p", "embedded-assets-probe"]
            if initialized:
                command.append("--locked")
            if release:
                command.append("--release")
            result = subprocess.run(
                [*command, "--", key], cwd=fixture, env=environment,
                capture_output=True, text=True,
            )
            if failure:
                assert result.returncode != 0, "release accepted missing Console output"
                assert "Console release assets are missing" in result.stderr, result.stderr
            elif result.returncode != 0:
                raise RuntimeError(result.stderr)
            initialized = True
            return result.stdout

        assert run() == "console-build-A"
        (dist / "index.html").write_text("console-build-B")
        (dist / "new.txt").write_text("new-asset")
        (dist / "old.txt").unlink()
        assert run() == "console-build-B", "incremental release retained stale HTML"
        assert run(key="new.txt") == "new-asset", "new asset was not embedded"
        assert run(key="old.txt") == "<missing>", "deleted asset remained embedded"
        assert "Vite dev server" in run(release=False)
        assert run() == "console-build-B", "debug output contaminated release output"
        (dist / "index.html").unlink()
        run(failure=True)
        (dist / "index.html").write_text("")
        run(failure=True)
        dist.rename(dist.with_name("hidden-dist"))
        run(failure=True)
        assert "Vite dev server" in run(release=False)
        assert not (assets / "assets").exists(), "build wrote generated files into crate sources"
        print("Embedded assets: content/addition/removal/profile/missing-output checks passed")


if __name__ == "__main__":
    main()
