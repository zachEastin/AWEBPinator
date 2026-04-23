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

## Manual Test Checklist

Use this when validating the app locally after a change.

1. Start the app with `cargo run`.
2. Import a handful of `.png` or `.jpg` files through the file picker.
3. Drag image files from the file manager into the timeline and confirm they import.
4. Select one or more frames and verify:
   - duration changes apply
   - duplicate, copy/paste, and remove work
   - move up/down works
   - drag reorder works
5. In the selection editor, verify:
   - rotate left/right works
   - flip horizontal/vertical works
   - crop/resize numeric fields update the selected-frame preview
6. Confirm the selected-frame preview updates after selection changes and transform edits.
7. Use the loop actions:
   - loop duplicate
   - loop reverse
   - ping-pong
8. Set an output path and export an animated WebP.
9. Verify the exported file exists and opens in an image viewer/browser that supports animated WebP.
10. Save a project, reopen it, and confirm the timeline, durations, and export settings are restored.

## VS Code

This repo includes VS Code workspace settings under `.vscode/`.

Useful editor workflows:

- Build task: `cargo build`
- Test task: `cargo test`
- Debug launch: `Debug AWEBPinator`
- Testing panel: Rust tests discovered by `rust-analyzer`

See [.vscode/README.md](./.vscode/README.md) for the editor-specific notes.

# Credits

Used GPT-5.4 and GPT-5.5 to create this.
