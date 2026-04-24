# Workflows

Run all commands from the repo root.

## Inspect Before Editing

Recommended first commands:

```bash
git status --short
rg --files -g '!*target*'
```

Then inspect the files directly related to the requested change. Use `rg` for symbol and text search before making assumptions.

## Build

```bash
cargo build
```

For release builds:

```bash
cargo build --release
```

## Test

```bash
cargo test
```

Run Clippy with warnings denied:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## Run

```bash
cargo run
```

For a bounded startup check:

```bash
timeout 5s cargo run
```

Use the bounded run when you need to confirm startup without leaving the GTK app running.

## Verification Expectations

Use a local-first GUI testing ladder:

- Tier 0 logic checks: `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings`.
- Tier 1 GTK startup smoke: `cargo build` and `timeout 5s cargo run`.
- Tier 2 manual GUI checklist: run the workflow below for visible UI behavior.
- Tier 3 optional AT-SPI smoke: `python3 tests/gui/smoke.py` after `cargo build`.

Treat Tier 1 as passing only when `timeout 5s cargo run` is stopped by the timeout, usually exit code `124`, and the output stays free of panic, abort, or new GTK warnings introduced by the change.

Tier 3 requires a running graphical session and Fedora packages `python3-dogtail` and `at-spi2-core`. Do not treat it as part of normal `cargo test`.

For non-UI, model-level changes, normally run:

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

For UI-affecting changes, run at minimum:

```bash
cargo build
cargo test
timeout 5s cargo run
```

If the change affects visual layout or interaction-heavy behavior, prefer a real interactive `cargo run` over only `timeout 5s cargo run`.

UI-affecting areas include:

- GTK layout.
- Selection behavior.
- Drag/drop or reorder behavior.
- Preview rendering.
- Startup/init code.
- File import flows.
- Export controls.

If `cargo run` cannot be verified because there is no GTK display, state that explicitly in the final notes and rely on build/test/clippy as partial verification.

## Optional AT-SPI GUI Smoke

After `cargo build`, run:

```bash
python3 tests/gui/smoke.py
```

The smoke script should only verify that the app launches, the main window appears, and key accessible controls are discoverable. Keep interaction-heavy validation in the manual checklist until the GUI automation has stable fixtures and stronger coverage.

## Manual UI Checklist

Use this after interaction or export changes:

1. Start the app with `cargo run`; expect the window to open without GTK/display errors.
2. Import three small `.png` or `.jpg` files through the picker; expect three timeline tiles and a selected-frame preview.
3. Drag image files from the file manager into the timeline; expect the append/prepend/replace prompt when frames already exist.
4. Verify single selection, Ctrl multi-selection, and Shift range selection; expect selected tiles to show the full blue selected background.
5. Verify duration edits, duplicate, copy/paste, remove, move up/down, and drag reorder; expect coherent timeline order and selection.
6. Verify rotate, flip, crop, resize, and fit-mode edits update the preview.
7. Visit Edit, Loop, Export, and Diagnostics; expect each tab to show controls without overlapping or clipped text.
8. Verify `Create Loop` appends only the mirrored interior frames and lengthens the original source endpoints.
9. Export an animated WebP and confirm the output exists.
10. Save and reload a project and confirm frames, order, durations, transforms, and export settings persist.

## Documentation Updates

Update docs when behavior or workflow changes:

- `README.md`: user-facing build/run/test or feature behavior.
- `AGENTS.md`: agent-critical workflow or safety rules.
- `.vscode/README.md`: VS Code workflow changes.
- `.ai/*`: AI workflow, architecture, or tracker changes.

## Task Tracker Workflow

Every task should update `.ai/task_tracker.md`:

1. Add an entry under `Active Tasks` before implementation.
2. Record relevant files inspected.
3. Move the entry to `Completed Tasks` when done.
4. Include verification commands and results.
5. Note uncertainty or skipped verification.

Keep entries short. This is a coordination log, not a full changelog.
