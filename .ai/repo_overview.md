# Repository Overview

## What This Repo Is

AWEBPinator is a native Linux desktop app for assembling still images into animated WebP files.

Current implementation:

- Language/runtime: Rust, edition 2024.
- UI framework: GTK4 through `gtk4` plus Relm4.
- Image processing: Rust `image` crate for thumbnails, previews, transforms, and rendered export frames.
- Export backend: system `ffmpeg` using the `libwebp_anim` encoder.
- Diagnostics: system `ffmpeg` and `ffprobe` availability checks.
- Project persistence: pretty JSON via `serde` and `serde_json`.

The app targets Fedora Linux. Required local tools are `cargo`, GTK4 development libraries, `ffmpeg`, and `ffprobe`.

## Top-Level Layout

- `Cargo.toml`: package metadata and Rust dependencies.
- `Cargo.lock`: locked Rust dependency graph.
- `README.md`: user-facing build, run, test, and manual testing instructions.
- `AGENTS.md`: general agent instructions and repo-specific warnings.
- `.ai/`: repo-local AI workflow and context system.
- `.vscode/`: editor tasks, launch config, settings, and workflow notes.
- `src/`: application source.

## Source Layout

- `src/main.rs`: binary entrypoint; initializes tracing and starts the app.
- `src/lib.rs`: module exports.
- `src/app.rs`: Relm4 component, GTK layout, message handling, dialogs, shortcuts, timeline tile rendering, and CSS.
- `src/types.rs`: shared data types for frames, transforms, export profiles, jobs, and project documents.
- `src/timeline.rs`: frame list model and timeline operations such as import, duplicate, paste, reorder, duration changes, and loop generation.
- `src/selection.rs`: click, Ctrl-click, Shift-click, and Ctrl-Shift selection behavior.
- `src/thumbnail.rs`: cache directory creation, image metadata, thumbnails, previews, transforms, resize/crop/fit behavior, and frame rendering for export.
- `src/export.rs`: concat manifest generation, ffmpeg command construction, command preview, and export execution.
- `src/project.rs`: JSON project save/load.
- `src/runtime.rs`: ffmpeg/ffprobe diagnostics.

## Current User-Facing Features

- Import image frames from file picker or drag/drop.
- Horizontal bottom timeline with thumbnail tiles.
- Select, multi-select, duplicate, copy/paste, remove, move, and drag-reorder frames.
- Batch duration edits.
- Transform selected frames with rotation, flips, crop, resize, and fit modes.
- Preview selected frame.
- Export animated WebP with presets, quality, lossless, encoder preset, loop count, overwrite behavior, and advanced ffmpeg args.
- Save and load project JSON.

## Existing Tests

Tests currently cover:

- Timeline ordering and operations in `src/timeline.rs`.
- Selection behavior in `src/selection.rs`.
- Project save/load round trip in `src/project.rs`.
- Export manifest generation, command building, and WebP generation in `src/export.rs`.

There are no automated GTK interaction tests in the current tree. UI-affecting work requires at least a brief app run when possible.
