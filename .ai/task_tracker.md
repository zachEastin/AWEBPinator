# Task Tracker

Future agents must update this file during work.

## Status Key

- `Planned`: accepted but not started.
- `In Progress`: currently being implemented.
- `Blocked`: waiting on user input, environment, or external dependency.
- `Done`: completed and verified as far as practical.

## Active Tasks

No active tasks.

## Completed Tasks

### 2026-04-23 - Create repo-local AI workflow system

- Status: Done
- Request: Create a minimal `.ai/` context system for repeatable AI-assisted development.
- Files inspected: `Cargo.toml`, `README.md`, `AGENTS.md`, `.vscode/README.md`, `.vscode/tasks.json`, `.vscode/launch.json`, `.vscode/settings.json`, `.vscode/extensions.json`, `src/main.rs`, `src/lib.rs`, `src/app.rs`, `src/types.rs`, `src/timeline.rs`, `src/selection.rs`, `src/export.rs`, `src/project.rs`, `src/runtime.rs`, `src/thumbnail.rs`, `.gitignore`
- Files changed: `.ai/agent.md`, `.ai/repo_overview.md`, `.ai/architecture.md`, `.ai/workflows.md`, `.ai/task_tracker.md`, `AGENTS.md`
- Verification: Documentation-only change; verified by file inspection and `git status --short`.
- Notes: Existing `src/app.rs` had uncommitted changes before this task and was not modified.

### 2026-04-23 - Fix image drop import flow

- Status: Done
- Request: Make dragging image files onto the window work, and prompt for append, prepend, or replace when importing into a non-empty timeline.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `Cargo.toml`, `src/app.rs`, `src/timeline.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/timeline.rs`
- Verification: `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: Added window-content drop targets for `gdk::FileList`, `gio::File`, and URI text drops. Interactive desktop drag-and-drop behavior was not manually exercised in this environment.

### 2026-04-23 - Add playback and timeline navigation

- Status: Done
- Request: Add timeline playback in the preview area with spacebar, plus navigation buttons and keymaps for beginning/back/play/forward/end.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/timeline.rs`, `src/thumbnail.rs`, `src/types.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: Playback advances the selected frame through the preview using per-frame duration timing. Interactive keyboard and button behavior was not manually exercised in this environment.

### 2026-04-23 - Refine playback transport behavior

- Status: Done
- Request: Keep space bound to playback, move transport controls to the top of the timeline with icons, and make preview updates visible during playback.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: The global key handler now uses capture phase so `Space` triggers playback before focused buttons can open dialogs. Preview updates now switch immediately to a cached frame image while the async full preview render catches up.

### 2026-04-23 - Implement higher-res preview playback

- Status: Done
- Request: Render preview playback at least at the current UI resolution instead of the fixed low-resolution preview cap.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: Preview rendering now targets the current preview widget size multiplied by GTK scale factor, tracks stale preview jobs, and caches preview files by frame id plus render size. Interactive playback and resize quality were not manually exercised in this environment.

### 2026-04-23 - Fix low-res playback preview fallback

- Status: Done
- Request: Stop playback from flashing the low-resolution thumbnail proxy while high-resolution preview renders catch up.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`, `src/types.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: Preview cache filenames now include the frame transform and target render size, playback reuses cached full previews immediately, and upcoming playback frames are prewarmed instead of falling back to the 160 px timeline thumbnail.

### 2026-04-23 - Fix blank first-visit playback preview

- Status: Done
- Request: Prevent the preview area from going blank the first time a frame is visited during playback.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Files changed: `.ai/task_tracker.md`, `src/thumbnail.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: Preview cache files are now written to a temporary file and atomically renamed into place so playback never treats a partially-written prewarmed PNG as a ready cache hit.

## Parking Lot

- Consider adding automated GTK smoke or interaction tests if the project later adopts a GUI testing strategy.
- Consider documenting project JSON schema/versioning if saved-project compatibility becomes important.
