# Task Tracker

Future agents must update this file during work.

## Status Key

- `Planned`: accepted but not started.
- `In Progress`: currently being implemented.
- `Blocked`: waiting on user input, environment, or external dependency.
- `Done`: completed and verified as far as practical.

## Active Tasks

### 2026-04-23 - Polish section headers and summary cards

- Status: Done
- Request: Continue the visual polish pass by improving spacing, typography, shadows, section headers, and summary cards so the workflow panels feel closer to the target screenshots.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Section cards now use icon-accented headers, summary labels have a richer inset-card treatment, and shells/cards/headings use stronger spacing and shadow styling. The page-heading subtitle width constraint was moved into widget properties after GTK rejected a CSS `max-width` rule.

### 2026-04-23 - Restore colored icons and enlarge preset cards

- Status: Done
- Request: Add colored icons again and make the preset cards taller and more square with larger icons to better match the target screenshots.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Added reusable icon tone classes and updated the shared button/card builders so tabs and actions can carry colored icons, while preset choice cards now use centered larger icons and taller card proportions.

### 2026-04-23 - Add icon-rich workflow buttons

- Status: Done
- Request: Add nicer icons across the UI so the workflow shell better matches the visual direction from the target screenshots.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `AGENTS.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Added symbolic icons to the top actions, workflow tabs, quick-edit controls, crop cards, loop cards, export preset cards, export actions, diagnostics action, and timeline toolbar using shared labeled-button builders so the icon treatment stays consistent.

### 2026-04-23 - Add guided crop workflow

- Status: Done
- Request: Continue the redesign by turning the Edit page crop control into a guided workflow instead of only routing users into Advanced mode.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/preferences.rs`, `src/thumbnail.rs`, `src/types.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: The Edit page now offers guided crop presets with start/center/end anchoring plus apply and clear actions. Guided crops work from the current cropped area when one already exists, while the Advanced numeric crop controls remain available for fine tuning.

### 2026-04-23 - Persist advanced mode and upgrade export preview

- Status: Done
- Request: Continue the redesign by persisting UI-only workflow preferences and making the Export tab preview reflect export sizing instead of the generic frame preview.
- Files inspected: `.ai/task_tracker.md`, `Cargo.toml`, `src/app.rs`, `src/lib.rs`, `src/preferences.rs`, `src/project.rs`, `src/thumbnail.rs`, `src/types.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/lib.rs`, `src/preferences.rs`, `src/thumbnail.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Advanced mode now persists in a UI preferences file outside the project JSON, and Export Preview now renders through its own cache path keyed by export sizing and fit mode so the Export tab can show a more accurate result.

### 2026-04-23 - Improve default fit and responsive scaling

- Status: Done
- Request: Continue the redesign by making the default window fit better on screen and adding simple region-based responsive scaling.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Reduced the default window size and added a compact layout mode keyed off window width. On narrower windows, the main workspace, Loop/Export side rails, and timeline toolbar now reflow to stacked layouts with smaller preview size requests so the app fits on screen more comfortably.

### 2026-04-23 - Refine tab-specific workflow layouts

- Status: Done
- Request: Continue the beginner-first redesign by giving Loop and Export dedicated preview/summary layouts instead of relying on the shared generic preview column.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Loop and Export now own their preview cards and summary rails, while Edit keeps the larger dedicated preview column. Preview resize messages are tab-aware so render sizing follows the visible workflow surface instead of one generic widget.

### 2026-04-23 - Begin beginner-first workflow shell redesign

- Status: Done
- Request: Start implementing the new beginner-first GUI shell with workflow tabs, Advanced mode, persistent timeline, and calmer visual hierarchy.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `Cargo.toml`, `src/app.rs`, `src/types.rs`, `src/timeline.rs`, `src/selection.rs`, `src/export.rs`, `src/runtime.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/timeline.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Landed the first redesign slice with a tabbed Edit/Loop/Export/Diagnostics shell, shared preview plus persistent timeline layout, an Advanced toggle that reveals expert controls inline, export and loop summary panels, and reusable timeline loop-source helpers for guided loop creation. This slice keeps the existing project format and core non-UI logic intact.

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
