# AWEBPinator Agent Notes

This file is for future agents working in this repository.

## Project Summary

- Desktop app for Linux/Fedora
- Rust + GTK4 + Relm4
- Uses system `ffmpeg` and `ffprobe` to export animated WebP and MP4 files
- Main code lives in `src/`

## Environment Expectations

- OS target: Fedora Linux
- Required tools:
  - `cargo`
  - `ffmpeg`
  - `ffprobe`
  - GTK4 development libraries

If the app fails very early during startup, check for GTK/display-related initialization issues first.

## Core Commands

Run these from repo root:

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Run the app:

```bash
cargo run
```

If you need a bounded verification run from the terminal, use:

```bash
timeout 5s cargo run
```

That is useful for confirming startup regressions without leaving a GTK process running forever. Treat it as a pass only when `timeout` stops the app, typically with exit code `124`, and the output contains no panic, abort, or new GTK warnings tied to the change.

## Verification Requirements

Do not treat `cargo test` alone as sufficient for UI changes.

Use the local-first GUI testing ladder:

- Tier 0 logic checks: `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings`
- Tier 1 GTK startup smoke: `cargo build` and `timeout 5s cargo run` with no panic/abort output and a timeout-driven exit
- Tier 2 manual GUI checklist: import, selection, drag/drop, reorder, preview, transform, loop, export, save/load
- Tier 3 optional local AT-SPI smoke: `python3 tests/gui/smoke.py`

Tier 3 requires a graphical session plus Fedora packages `python3-dogtail` and `at-spi2-core`. It is not part of normal `cargo test`.

If you change any of the following, you should run the app at least briefly:
- GTK layout
- selection behavior
- drag/drop or reorder behavior
- preview rendering
- startup/init code
- file import flows
- export controls

Minimum expected verification for UI-affecting changes:

1. `cargo build`
2. `cargo test`
3. `cargo run` or `timeout 5s cargo run`

If the change is specifically visual or interaction-heavy, prefer a real interactive run over only `timeout`.

## Current UI Shape

- Main preview/editor occupies the main body
- Timeline is a horizontal thumbnail strip along the bottom
- Timeline loop creation is a single mirrored-loop action that holds the original source endpoints slightly longer
- Timeline tiles should currently show only:
  - preview
  - frame number
  - original filename
- Selection is shown by a blue background on the full tile area
- Reordering is intended to happen by dragging the tile itself

When editing the timeline UI, preserve that interaction model unless the user explicitly asks to change it.

## Accessibility And Test Handles

- Keep important controls discoverable by stable accessible labels.
- Icon-only buttons should have accessible labels matching their tooltip/action.
- Major controls should keep human-readable labels for AT-SPI/Dogtail smoke tests:
  - import/open/save
  - workflow tabs
  - Advanced mode
  - export output path and export action
  - transport controls
  - timeline edit controls
- Timeline tiles should keep deterministic accessible labels in the form `Frame 001 filename.png`.

## Repo-Specific Notes

- This repo has a local AI workflow system under `.ai/`. Future agents should start with `.ai/agent.md` and keep `.ai/task_tracker.md` updated while working.
- CSS setup must happen after GTK has a display. Do not call `relm4::set_global_css(...)` before GTK app/window initialization.
- The current project includes VS Code configs under `.vscode/` for build/test/debug tasks.
- `cargo build --release` now generates desktop-install artifacts under `target/release/`, including `awebpinator.desktop`, `icon.png`, and `install-awebpinator.sh` for current-user Fedora app installation/update.
- Use repo-local docs as the source of truth:
  - `README.md` for user-facing build/run/test steps
  - `.vscode/README.md` for editor workflow notes

## When Updating Docs

If you change build/test/run expectations or verification workflow, update:
- `README.md`
- `AGENTS.md`

If the change affects VS Code workflows, also update:
- `.vscode/README.md`
