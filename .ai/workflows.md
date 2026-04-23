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

## Manual UI Checklist

Use this after interaction or export changes:

1. Start the app with `cargo run`.
2. Import several `.png` or `.jpg` files through the picker.
3. Drag image files from the file manager into the timeline.
4. Verify select, multi-select, duration edits, duplicate, copy/paste, remove, move up/down, and drag reorder.
5. Verify rotate, flip, crop, resize, and fit-mode edits update the preview.
6. Verify loop duplicate, loop reverse, and ping-pong.
7. Export an animated WebP and confirm the output exists.
8. Save and reload a project and confirm frames, durations, transforms, and export settings persist.

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
