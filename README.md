# AWEBPinator

AWEBPinator is a native Linux desktop app for assembling still images into animated WebP files with `ffmpeg`.

The current app is written in Rust with GTK4/Relm4 and supports:

- importing image frames from the file picker or drag/drop
- timeline ordering, selection, duplicate, copy/paste, remove, and duration edits
- basic non-destructive transforms on selected frames
- animated WebP export through `ffmpeg`
- project save/load

## Requirements

- Fedora Linux
- Rust toolchain with `cargo`
- GTK4 development libraries
- `ffmpeg`
- `ffprobe`

On Fedora, the main runtime/build dependencies are typically already available if you have a Rust + GTK workstation setup. If not, install at least:

```bash
sudo dnf install gtk4-devel ffmpeg
```

## Build

From the repo root:

```bash
cargo build
```

For an optimized build:

```bash
cargo build --release
```

## Run

Run the app directly with Cargo:

```bash
cargo run
```

Or run the compiled debug binary:

```bash
./target/debug/awebpinator
```

For the release binary:

```bash
./target/release/awebpinator
```

## Test

Run the Rust test suite:

```bash
cargo test
```

Run linting with Clippy:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## GUI Testing Strategy

AWEBPinator uses a local-first GUI testing ladder:

| Tier | Purpose | Command or method |
| --- | --- | --- |
| 0 | Fast logic checks for model/export/project behavior | `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings` |
| 1 | GTK startup smoke check | `cargo build` then `timeout 5s cargo run` |
| 2 | Human validation of visible workflows | follow the manual checklist below |
| 3 | Optional local AT-SPI smoke automation | `python3 tests/gui/smoke.py` after `cargo build` |

The optional AT-SPI smoke script is not part of `cargo test` because it needs a running graphical session. On Fedora, install the local automation dependencies with:

```bash
sudo dnf install python3-dogtail at-spi2-core
```

The first automated smoke is intentionally small: it launches the app, confirms the main window and key accessible controls are discoverable, then closes the app. Use it as a supplement to the manual checklist, not a replacement for visual review.

## Manual Test Checklist

Use this when validating the app locally after a change.

1. **Launch**: Start the app with `cargo run`; expect the AWEBPinator window to open without GTK/display errors.
2. **Import**: Import three small `.png` or `.jpg` files through the file picker; expect three timeline tiles and a selected-frame preview.
3. **Drag/drop**: Drag image files from the file manager into the timeline; expect the app to offer append, prepend, or replace when frames already exist.
4. **Selection**: Select one frame, Ctrl-select multiple frames, and Shift-select a range; expect the selected timeline tiles to show the full blue selected background.
5. **Timeline edits**: Verify duration changes, duplicate, copy/paste, remove, move up/down, and drag reorder; expect the timeline order and selection state to remain coherent.
6. **Preview and transforms**: Rotate, flip, crop, resize, and change fit mode; expect the selected-frame preview to update after each edit.
7. **Workflow tabs**: Visit Edit, Loop, Export, and Diagnostics; expect each tab to show its controls without overlapping or clipped text.
8. **Loop actions**: Run duplicate, reverse, and ping-pong loop actions; expect the timeline to update according to the selected loop source.
9. **Export**: Set an output path and export an animated WebP; expect the output file to exist and open in an image viewer/browser that supports animated WebP.
10. **Project persistence**: Save a project, reopen it, and confirm frames, order, durations, transforms, and export settings are restored.

## VS Code

This repo includes VS Code workspace settings under `.vscode/`.

Useful editor workflows:

- Build task: `cargo build`
    - Debug launch: `Debug AWEBPinator`
- Testing panel: Rust tests discovered by `rust-analyzer`

See [.vscode/README.md](./.vscode/README.md) for the editor-specific notes.

# Credits

Used GPT-5.4 and GPT-5.5 to create this.
