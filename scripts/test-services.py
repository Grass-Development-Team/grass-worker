#!/usr/bin/env python3
"""Run database/cache regressions against explicitly configured disposable services."""

import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    required = ("GRASS_TEST_DATABASE_URL", "GRASS_TEST_REDIS_URL")
    missing = [key for key in required if not os.environ.get(key)]
    if missing:
        raise SystemExit("Missing integration test configuration: " + ", ".join(missing))

    environment = os.environ.copy()
    environment["GWAPI_DATABASE_URL"] = environment["GRASS_TEST_DATABASE_URL"]
    with tempfile.TemporaryDirectory(prefix="grass-service-regressions-") as directory:
        subprocess.run(
            ["cargo", "run", "--locked", "-p", "grass-control-api", "--", "--config",
             str(Path(directory) / "migration.toml"), "migrate"],
            cwd=ROOT, env=environment, check=True,
        )
        # All ignored Control API cases require PostgreSQL/Redis except Chromium.
        # Selecting the complete set also makes newly added service cases visible.
        subprocess.run(
            ["cargo", "test", "--locked", "-p", "grass-control-api", "--", "--ignored",
             "--skip", "chromium_captures_a_1280_by_720_png"],
            cwd=ROOT, env=environment, check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "-p", "grass-cache", "redis_backend::tests::",
             "--", "--ignored"],
            cwd=ROOT, env=environment, check=True,
        )


if __name__ == "__main__":
    main()
