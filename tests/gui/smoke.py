#!/usr/bin/env python3
"""Local AT-SPI smoke test for AWEBPinator.

Run from the repo root after `cargo build`:

    python3 tests/gui/smoke.py

This is intentionally outside `cargo test` because it requires a graphical
session plus Fedora packages `python3-dogtail` and `at-spi2-core`.
"""

from __future__ import annotations

import contextlib
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

try:
    from dogtail.tree import root
except ModuleNotFoundError:
    print(
        "Missing dogtail. Install it on Fedora with: "
        "sudo dnf install python3-dogtail at-spi2-core",
        file=sys.stderr,
    )
    sys.exit(2)


REPO_ROOT = Path(__file__).resolve().parents[2]
APP_BINARY = REPO_ROOT / "target" / "debug" / "awebpinator"
APP_NAMES = {"awebpinator", "AWEBPinator"}
REQUIRED_ACCESSIBLE_LABELS = {
    "Import Images",
    "Open Project",
    "Save Project",
    "Edit workflow tab",
    "Loop workflow tab",
    "Export workflow tab",
    "Diagnostics workflow tab",
    "Advanced mode",
    "Go to beginning (Ctrl+Left)",
    "Play or pause preview playback (Space)",
}
KNOWN_ATSPI_WARNING_PARTS = (
    "dbind-WARNING",
    "AT-SPI: Error in GetItems",
    "/org/a11y/atspi/cache",
)


@contextlib.contextmanager
def filter_known_atspi_stderr():
    """Hide the common AT-SPI cache warning while preserving other stderr."""
    original_stderr = os.dup(2)
    with tempfile.TemporaryFile(mode="w+b") as captured:
        os.dup2(captured.fileno(), 2)
        try:
            yield
        finally:
            os.dup2(original_stderr, 2)
            os.close(original_stderr)
            captured.seek(0)
            for raw_line in captured.read().decode(errors="replace").splitlines():
                if not raw_line.strip():
                    continue
                if all(part in raw_line for part in KNOWN_ATSPI_WARNING_PARTS):
                    continue
                print(raw_line, file=sys.stderr)


def iter_nodes(node, max_depth=12):
    if max_depth < 0:
        return
    yield node
    try:
        children = list(node.children)
    except Exception:
        return
    for child in children:
        yield from iter_nodes(child, max_depth - 1)


def node_name(node) -> str:
    try:
        return node.name or ""
    except Exception:
        return ""


def find_app():
    for _ in range(80):
        for app in root.children:
            names = {node_name(node) for node in iter_nodes(app, max_depth=3)}
            if APP_NAMES.intersection(names):
                return app
        time.sleep(0.1)
    raise RuntimeError("AWEBPinator did not appear in the AT-SPI tree")


def visible_names(app) -> set[str]:
    return {name for node in iter_nodes(app) if (name := node_name(node))}


def main() -> int:
    if not APP_BINARY.exists():
        print("Missing debug binary. Run `cargo build` first.", file=sys.stderr)
        return 2

    proc = subprocess.Popen([str(APP_BINARY)], cwd=REPO_ROOT)
    try:
        with filter_known_atspi_stderr():
            app = find_app()
            names = visible_names(app)
        missing = sorted(REQUIRED_ACCESSIBLE_LABELS - names)
        if missing:
            print("Missing accessible labels:", file=sys.stderr)
            for label in missing:
                print(f"  - {label}", file=sys.stderr)
            return 1
        print("AWEBPinator GUI smoke passed.")
        return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)


if __name__ == "__main__":
    sys.exit(main())
