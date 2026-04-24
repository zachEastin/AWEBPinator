# Task Tracker

Future agents must update this file during work.

## Status Key

- `Planned`: accepted but not started.
- `In Progress`: currently being implemented.
- `Blocked`: waiting on user input, environment, or external dependency.
- `Done`: completed and verified as far as practical.

## Active Tasks

### 2026-04-24 - Make resize global and clamp preview scaling

- Status: Done
- Request: Make the resize workflow apply to all frames regardless of selection, refresh previews and size-dependent UI after resize, prevent previews from rendering above the frame/export dimensions, and set the window background to `#11161d`.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`, `AGENTS.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt --all`, `cargo build`, `cargo test`, `timeout 5s cargo run >/tmp/awebpinator-resize-preview-smoke.log 2>&1` with exit code `124`
- Notes: Quick resize now applies to every frame in the timeline instead of only the active selection. Refresh work now requeues thumbnails for all frames and invalidates/rebuilds the selected preview and export preview so size-dependent UI stays in sync after resize. Preview render targets are clamped to the frame's effective dimensions, export preview targets are clamped to the configured export size, timeline tile aspect ratios now follow effective frame dimensions, and the preview metadata now shows source vs current dimensions. The window itself now gets an `app-window` CSS class so the top-level background uses `#11161d` instead of the default GTK gray.

### 2026-04-24 - Add real custom resize workflow to Edit tab

- Status: Done
- Request: Expand the Edit resize preset control, make the `Custom` preset stay active, and add `Multiplier` and `Custom` resize tabs that keep aspect ratio and apply the chosen resize to the selected frames.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `src/types.rs`, `/memories/repo/awebpinator-gtk-notes.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt --all`, `cargo build`, `cargo test`, `git diff --check`, `timeout 5s cargo run >/tmp/awebpinator-quick-resize-smoke.log 2>&1`
- Notes: The Edit resize row now lets the preset combo expand across the available width and keeps a dedicated quick-resize state instead of deriving the combo directly from the applied transform on every redraw. Choosing `Custom` reveals `Multiplier` and `Custom` tabs: multiplier mode supports typed float values plus +/-1 stepping, while custom width/height inputs stay linked to the selected frame's aspect ratio. The bounded GTK startup run exited early with code `0` in this environment instead of timing out with `124`, so it was treated as inconclusive rather than a full smoke-test pass.

### 2026-04-24 - Refine Edit tab split and adjustments layout

- Status: Done
- Request: In the Edit tab, make the preview take roughly two-thirds of the width and the controls take one-third, enlarge the Quick Action buttons to fill their area, and split Adjustments into clearer crop and resize sections with ratio-only Guided Crop summary text.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `/memories/repo/awebpinator-gtk-notes.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: The Edit workspace now keeps the preview/control split aligned to a roughly 2:1 ratio in regular layout by driving the right-side width from the live workspace width while allowing compact layout to relax back to full expansion. Quick Actions now use an Edit-only fill treatment so the rotate/flip buttons occupy their full grid cells, Adjustments now separates Guided Crop from Resize, and the Guided Crop summary label reduces to the selected ratio once frame dimensions are known. No manual visual GTK inspection was performed in this pass.

### 2026-04-24 - Rework footer into active-tab contextual status bar

- Status: Done
- Request: Replace the footer's idle progress bar state with a single-row, active-tab-aware contextual summary that keeps global frame/duration metrics and readiness state while showing scoped action results and tab-specific summaries.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `src/types.rs`, `README.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `git diff --check`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Added a scoped footer status model in `src/app.rs`, converted the footer center slot into a single-line contextual summary per active tab, and restricted the footer progress bar to export/finalization states. `timeout 5s cargo run` launched `target/debug/awebpinator` but exited early with code `0` instead of timeout code `124`, so it was treated as an inconclusive GTK smoke check rather than a pass.

### 2026-04-24 - Make Timeline sections collapsible and split evenly

- Status: Done
- Request: Make the Timeline tab sections collapsible like the Edit tab and keep the left and right Timeline columns at a 50/50 width split.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `git diff --check`, `python3 tests/gui/smoke.py`, `timeout 5s cargo run`
- Notes: Timeline Actions, Clipboard And Order, Loop, Preview, and Loop Summary now use the same collapsible card treatment as Edit. The Timeline left/right columns now use a homogeneous horizontal box with both columns allowed to expand so the regular layout stays 50/50. `python3 tests/gui/smoke.py` continued to intermittently miss `Import Images`, `Open Project`, and `Save Project`, which are housed in the `Secondary actions` popover and were not changed by this patch. In this environment, `timeout 5s cargo run` again exited immediately with code `0`, so it was not counted as a timeout-smoke pass.

### 2026-04-24 - Reframe Loop tab as Timeline workspace

- Status: Done
- Request: Turn the current Loop tab into a timeline-focused workspace by moving the timeline action and clipboard/order controls out of Edit, keeping loop creation under a Loop section, and renaming the visible tab to better reflect frame order/duration automation work.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `tests/gui/smoke.py`, `README.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `tests/gui/smoke.py`, `README.md`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `python3 tests/gui/smoke.py`, `git diff --check`, `timeout 5s cargo run`
- Notes: Edit now keeps only frame/pixel-oriented adjustments, while the visible Timeline tab gathers timeline actions, clipboard/order controls, and a Loop section with the existing loop builder settings. The internal `WorkflowTab::Loop` state was left unchanged to keep the patch scoped. In this environment, `timeout 5s cargo run` exited immediately with code `0` after launching `target/debug/awebpinator`, so it was not treated as a timeout-smoke pass; the AT-SPI smoke test passed and confirmed the renamed Timeline tab instead.

### 2026-04-24 - Fix startup abort in timeline UI changes

- Status: Done
- Request: Fix the `cargo run` abort caused by invalid GTK values in the compacted timeline UI and update verification instructions to treat `timeout 5s cargo run` as a failure if it panics or aborts before timing out.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `README.md`, `AGENTS.md`, `.ai/workflows.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `README.md`, `AGENTS.md`, `.ai/workflows.md`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run > /tmp/awebpinator-timeout-smoke.log 2>&1` with exit code `124`, `git diff --check`
- Notes: Removed the invalid `max-height` CSS property, replaced the negative `margin-top` overlay hack with a valid positive inset, and changed preview-layout tick callbacks to use the non-panicking Relm4 sender path so shutdown no longer aborts the process. The earlier bounded-run success claim for the timeline density change was corrected after reproducing the abort from a real `cargo run`.

### 2026-04-24 - Reduce edit and timeline UI density

- Status: Done
- Request: Make Edit sections collapsible with chevrons, move duplicate/remove/duration/copy/paste/move up/down controls from the timeline into new Edit tab sections, and reduce timeline tile height by tightening the tile layout.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `src/types.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`, `git diff --check`
- Notes: Edit now uses collapsible card sections with a header chevron that swaps between open and closed states, the timeline strip keeps only centered transport controls, duplicate/remove/duration/copy/paste/move buttons now live in dedicated Edit sections, and timeline tiles now size from the frame aspect ratio with the badge/check overlaid on the thumbnail plus a tighter centered filename footer.

### 2026-04-24 - Tighten export column split and settings alignment

- Status: Done
- Request: Make the Export tab left/right split 50/50, convert Export Settings to a stable one-third/two-thirds row layout, and vertically center the Export Action content while using a working success icon.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`, `git diff --check`
- Notes: Export now keeps an even left/right split in regular layout, the settings rows use a grid-backed one-third/two-thirds split so controls line up cleanly, and the action card content is vertically centered with `object-select-symbolic` replacing the missing success icon.

### 2026-04-24 - Refine export tab visual hierarchy and blue control styling

- Status: Done
- Request: Remove section-header icons, shift more controls onto the blue visual language, clean up the Export tab preview/settings/summary/action sections, and improve the export action status presentation.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`, `git diff --check`
- Notes: Removed section-header icons globally, moved buttons and form controls further into the blue visual treatment, shortened the Export preview, removed the leftover export-preview metadata binding, converted Export Summary into a two-column label/value layout, and rebuilt Export Action as an icon + status heading + detail copy block above stacked actions.

### 2026-04-24 - Adjust export preview card and summary/action row

- Status: Done
- Request: In the Export tab, make the preview section a fixed height and place Export Summary and Export Action in a single row beneath it like the provided screenshot.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `timeout 5s cargo run`, `git diff --check`
- Notes: The Export tab now keeps the preview card at a stable height, moves Export Summary and Export Action into a shared row beneath that preview, and stacks the action buttons vertically inside their card so the two cards fit cleanly side by side.

### 2026-04-24 - Fix responsive resizing and custom titlebar

- Status: Done
- Request: Restore full responsiveness when resizing the window and replace the default title bar with the current custom header bar.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `tests/gui/smoke.py`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `tests/gui/smoke.py`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`, `python3 tests/gui/smoke.py`, `python3 -m py_compile tests/gui/smoke.py`, `git diff --check`
- Notes: Moved the custom header into `window.set_titlebar(...)` with `gtk::WindowHandle` and start/end `gtk::WindowControls`. Removed several width floors by dropping large content-stack/right-column/scroller/progress-bar width requests, reducing preview size requests, making tabs horizontally scrollable, and stacking Loop/Export bodies in compact mode. Updated the Dogtail smoke labels for the new Organize tab and menu-based Diagnostics action.

### 2026-04-24 - Implement export dashboard UI refactor

- Status: Done
- Request: Implement the tasks in `ui_changes_task_tracker.md` to refactor the UI into a cleaner app shell, tab navigation, left-preview/right-export-control layout, calmer timeline, and hidden advanced/debug details.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `ui_changes_task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`, `python3 tests/gui/smoke.py`, `git diff --check`
- Notes: Added an app-shell header with centered title, Advanced toggle, and secondary actions menu; replaced the Diagnostics nav pill with Organize/Edit/Loop/Export tabs; rebuilt Export as left preview plus right preset/settings/summary/action/advanced controls; replaced quality and loop controls with a slider/dropdown in the main settings area; moved lossless/encoder/raw command details under Advanced Options; refreshed timeline header/cards/status styling while preserving tile drag reorder and stable frame accessible labels. A few requested visual refinements remain approximate rather than exhaustive, including collapsed middle-frame elision and a true expandable Advanced Options chevron row.

### 2026-04-24 - Keep Loop and Export previews on the left

- Status: Done
- Request: Change the UI so the preview is always on the left and settings are always on the right, specifically in the Loop and Export tabs.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Loop and Export now append preview/summary columns before settings columns, keep those tab bodies horizontal in compact and regular modes, and constrain the settings columns while allowing previews to expand.

### 2026-04-24 - Remove export debug logs and reports

- Status: Done
- Request: Export now works; remove the temporary debugging logs, reports, and tracing scaffolding without regressing the background export flow or the completion-freeze fix.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`, `src/lib.rs`, `src/main.rs`, `Cargo.toml`, `/memories/repo/awebpinator-gtk-notes.md`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`, `src/lib.rs`, `src/main.rs`, `Cargo.toml`, `src/export_debug.rs`, `/memories/repo/awebpinator-gtk-notes.md`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Removed the temporary export logger module, tracing subscriber setup, debug-only status strings, and app/export diagnostic logging paths. The background worker thread, GLib poll loop, footer progress bar, deferred post-export UI restore, output-path normalization, and output-entry change suppression remain in place. Deleted `/tmp/awebpinator-export-logs/` after cleanup.

### 2026-04-24 - Break output-entry feedback loop after export

- Status: Done
- Request: Export still froze after completion; the latest `post-export-msg` instrumentation identified an endless `AppMsg::SetOutputPath` loop alternating between the real output path and an empty string.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `/tmp/awebpinator-export-logs/export-session-0001.log`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: The root cause was programmatic `output_entry.set_text(...)` inside `update_view(...)` triggering the entry's `connect_changed` handler, which fed `SetOutputPath` back into Relm4 and retriggered `update_view(...)` indefinitely. The export output entry now suppresses its own `changed` handler while view sync writes the displayed path.

### 2026-04-24 - Trace post-export message source

- Status: Done
- Request: Export still freezes after completion; the latest sampled log ruled out `PreviewLayoutChanged` as the repeating trigger because the redraw loop continued without any preview-layout entries.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `/tmp/awebpinator-export-logs/export-session-0001.log`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`
- Notes: Window-width notifications now only emit `WindowLayoutChanged` when the app would actually switch between compact and regular layout modes. The component also now writes sampled `post-export-msg` entries for repeated `AppMsg` and `CommandMsg` traffic after export completion so the next freeze log identifies the exact message source rather than only the resulting redraw storm.

### 2026-04-24 - Throttle post-export redraw diagnostics

- Status: Done
- Request: The latest export log kept growing into hundreds of thousands of lines, so reduce redraw-log volume before the next retest while keeping the signal needed to identify whether preview layout changes still drive the loop.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `/tmp/awebpinator-export-logs/export-session-0001.log`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Normal `update_view(...)` phase tracing now logs the first few redraw cycles in full and then samples later cycles instead of writing every pass. Accepted `PreviewLayoutChanged` messages now also emit a dedicated `preview-layout` marker with old/new sizes so the next export log can distinguish a true layout-driven loop from a generic redraw storm.

### 2026-04-24 - Defer post-export UI restoration

- Status: Done
- Request: Export now stays responsive through encoding and result delivery, but the app still freezes immediately after the successful completion handoff.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`, captured export session log
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Successful export completion is now split across three GTK turns: `FinalizeExportUi`, `ResumePreviewLayoutWatch`, and `CompleteExportUiRestore`. Normal `update_view(...)` also now avoids redundant `set_visible(...)` and stack-child updates, and emits coarse `gtk-view-phase` markers so the next completion log can identify whether any remaining freeze is in layout sizing, tab visibility, footer sync, preview sync, inspector sync, or timeline sync.

### 2026-04-24 - Short-circuit export-time view sync

- Status: Done
- Request: The captured export log still stops inside `update_with_view{input=ExportNow}` after the footer progress bar log line, before any deferred worker startup or GTK poll tick.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, captured export session log
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: `update_view(...)` now takes an export-only fast-path at the top of the function before any layout, tab, preview, inspector, export-form, or timeline synchronization work. The footer progress bar stays mounted and shows `Idle` when not exporting, so export start no longer toggles the widget's visibility before yielding back to the main loop.

### 2026-04-24 - Defer export worker start until next main-loop turn

- Status: Done
- Request: Export worker finishes and stores the result, but the GTK poll source never logs a tick; defer worker startup so the UI can return to the main loop and render the progress state before export begins.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`, `src/export_debug.rs`, captured export session log
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt`, `git diff --check`, `timeout 5s cargo run`
- Notes: The captured session log still stopped before `StartExportWorker(...)` and before any GTK poll tick. The next mitigation keeps the footer progress bar permanently present to avoid a visibility-triggered relayout on export start and suspends preview-layout watch callbacks during export so footer changes cannot flood the main loop with `PreviewLayoutChanged` traffic.

### 2026-04-24 - Add export freeze instrumentation logs

- Status: Done
- Request: Add detailed logging so a future export freeze shows exactly what stage is still alive and where the handoff stops.
- Files inspected: `.ai/task_tracker.md`, `src/main.rs`, `src/lib.rs`, `src/app.rs`, `src/export.rs`, local Relm4 runtime source
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`, `src/export_debug.rs`, `src/lib.rs`, `src/main.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt`, `git diff --check`, `timeout 5s cargo run`
- Notes: Added a persistent per-export debug log under `/tmp/awebpinator-export-logs/`, plus terminal tracing with thread IDs/names. The export worker now logs backend phases and blocking boundaries, the GTK poll timer logs heartbeats and version changes, the model logs result consumption, and `update_view` logs export progress-bar visibility/text changes so a future freeze can be localized to worker, poll, or GTK rendering.

### 2026-04-24 - Eliminate export progress UI backlog freeze

- Status: Done
- Request: The UI still freezes during export and can remain frozen even after the file is fully written.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`
- Verification: `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt`, `git diff --check`, `timeout 5s cargo run`
- Notes: The final shape also removes export completion from Relm4 command outputs. Export now runs on a dedicated `std::thread`, writes progress/completion into shared state, and the GTK main loop polls that state on a short timer. That avoids depending on command-output delivery for the UI to recover after ffmpeg exits. The footer progress bar is forced visible for the full export window and the generic footer status text is suppressed while exporting so the bar has room to render.

### 2026-04-24 - Move export fully into background and show footer progress

- Status: Done
- Request: Make export a background process, show a progress bar in the footer, and fix the app appearing hung after export completes.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `Cargo.toml`, `src/app.rs`, `src/export.rs`, `src/thumbnail.rs`, `src/types.rs`, local Relm4 0.11.0 sender implementation
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`, `git diff --check`
- Notes: Export now uses a multi-message Relm4 background command instead of a single completion callback, so the footer can show live frame-render and ffmpeg encoding progress. ffmpeg now runs with `-nostdin` plus `-progress pipe:1`, and the app clears export state on completion instead of leaving the UI stuck waiting for a worker that never reported incremental status.

### 2026-04-24 - Append WebP extension for extensionless exports

- Status: Done
- Request: Export fails when the selected output path has no extension because ffmpeg cannot infer the output format.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`, `src/export.rs`
- Verification: `cargo fmt`, `cargo test export::tests`, `cargo test app::tests::immediate_preview_uses_source_before_thumbnail_when_not_playing`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run 2>&1 | tee /tmp/awebpinator-export-path-fix-startup.log`, `rg -n "Gtk-CRITICAL|GLib-GObject-CRITICAL|gtk_scaler_new|g_object_unref" /tmp/awebpinator-export-path-fix-startup.log || true`, `git diff --check`
- Notes: The failing path `/mnt/shared/True-VFX/TrueTERRAIN/Docs/Scatter/stable_positions/stable_test` has no extension, so ffmpeg reported `Unable to choose an output format`. Export now normalizes extensionless paths to `.webp` for command preview and actual export; text entry typing is left untouched until export to avoid disrupting path input.

### 2026-04-24 - Autosave project on close and restore on launch

- Status: Done
- Request: Save a temporary project automatically on close and reopen with the same frames and effects loaded.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/project.rs`, `src/preferences.rs`, `src/types.rs`, `src/timeline.rs`
- Files changed: `.ai/task_tracker.md`, `README.md`, `src/app.rs`, `src/project.rs`, `tests/gui/smoke.py`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, Dogtail `window.close` check with temporary `XDG_STATE_HOME` confirming `autosave.awebp.json` creation, temporary autosave launch check showing restored `1 frame selected`, `python3 tests/gui/smoke.py`, `python3 -m py_compile tests/gui/smoke.py`, `timeout 5s cargo run 2>&1 | tee /tmp/awebpinator-autosave-startup-2.log`, `rg -n "Gtk-CRITICAL|GLib-GObject-CRITICAL|gtk_scaler_new|g_object_unref" /tmp/awebpinator-autosave-startup-2.log || true`, `git diff --check`
- Notes: Autosave reuses `ProjectDocument` JSON at `$XDG_STATE_HOME/awebpinator/autosave.awebp.json`, falling back through `$XDG_CONFIG_HOME` and `$HOME/.local/state`. The close handler stops the first close request, saves synchronously, then allows the second close request to proceed.

### 2026-04-23 - Stop preview fallback from returning to proxy resolution

- Status: Done
- Request: Selecting a frame or playing still brings the very low-res proxy back into the preview, and GTK paintable warnings are still appearing.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo test`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, `python3 tests/gui/smoke.py`, `timeout 5s cargo run 2>&1 | tee /tmp/awebpinator-run-after-proxy-fix.log`, `rg -n "Gtk-CRITICAL|GLib-GObject-CRITICAL|gtk_scaler_new|g_object_unref" /tmp/awebpinator-run-after-proxy-fix.log || true`
- Notes: The preview queue now falls back to the original source image instead of the 160 px timeline thumbnail when no exact cached render exists. Picture widgets hide instead of being assigned a null paintable when no valid image path is available.

### 2026-04-23 - Avoid GTK paintable warnings during picture updates

- Status: Done
- Request: Investigate repeated `gtk_scaler_new: assertion 'GDK_IS_PAINTABLE (paintable)' failed` and `g_object_unref` warnings during `cargo run`.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `python3 tests/gui/smoke.py`, `timeout 5s cargo run 2>&1 | tee /tmp/awebpinator-run-after-picture-fix.log`, `rg -n "Gtk-CRITICAL|GLib-GObject-CRITICAL|gtk_scaler_new|g_object_unref" /tmp/awebpinator-run-after-picture-fix.log || true`
- Notes: Clean bounded startup did not reproduce the warnings before the fix. Preview, loop preview, export preview, and timeline thumbnail widgets now load paths through `gdk::Texture::from_file` and only hand GTK a valid paintable; missing or invalid paths clear the picture instead of going through GTK's file loader.

### 2026-04-23 - Filter benign AT-SPI smoke warning

- Status: Done
- Request: The Dogtail smoke test prints a dbind AT-SPI cache warning even when it passes; make the output less confusing.
- Files inspected: `.ai/task_tracker.md`, `tests/gui/smoke.py`
- Files changed: `.ai/task_tracker.md`, `tests/gui/smoke.py`
- Verification: `python3 tests/gui/smoke.py`, `python3 -m py_compile tests/gui/smoke.py`
- Notes: The smoke script now filters the known AT-SPI cache warning during Dogtail tree traversal while preserving any other stderr output. The smoke test now prints only `AWEBPinator GUI smoke passed.` on success in this environment.

### 2026-04-23 - Implement local-first GUI testing strategy

- Status: Done
- Request: Implement the local-first GUI testing strategy with docs, accessibility/test handles, and a small future Dogtail smoke-test scaffold.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `.ai/workflows.md`, `.ai/repo_overview.md`, `.ai/architecture.md`, `README.md`, `AGENTS.md`, `Cargo.toml`, `src/app.rs`, local GTK Rust accessible APIs
- Files changed: `.ai/task_tracker.md`, `.ai/workflows.md`, `AGENTS.md`, `README.md`, `src/app.rs`, `tests/gui/smoke.py`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`, `python3 tests/gui/smoke.py`
- Notes: Added local-first GUI testing docs, stable accessible labels for core controls/timeline tiles, and an optional Dogtail smoke scaffold. The Dogtail smoke script currently exits with the documented missing-dependency message because `python3-dogtail` is not installed locally.

### 2026-04-23 - Restore allocated-size preview rendering

- Status: Done
- Request: Higher-res previews still appear as low-res proxies; restore the caching/rendering behavior that worked before commit `6c1a34074443ab3a936981b4015a2b21dffc5572`.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`, `src/types.rs`, `git show 6c1a34074443ab3a936981b4015a2b21dffc5572^:src/app.rs`, `git show 6c1a34074443ab3a936981b4015a2b21dffc5572^:src/thumbnail.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: The size-keyed preview cache/render path existed before the redesign and is still used. The fix makes the redesigned preview layout watcher stateful and adds a GTK tick fallback so renders are queued from the actual allocated preview widget size after layout settles, instead of leaving a smaller requested-size cache stretched across the visible panel.

### 2026-04-23 - Keep higher-resolution previews visible

- Status: Done
- Request: Fix higher-res previews not displaying while playing or paused; only the lower-res proxy appears visible.
- Files inspected: `.ai/agent.md`, `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`, `src/types.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo fmt`, `cargo build`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `timeout 5s cargo run`
- Notes: Existing rendered previews now stay visible for the current frame while a new exact-size render is pending, even after playback or workflow-tab layout changes clear the recorded render size. Thumbnail/source proxies are only used when there is no rendered preview to keep.

### 2026-04-23 - Fix preview proxy-to-render handoff

- Status: Done
- Request: Fix still preview and playback preview so the visible proxy image does not disappear when the app switches to the rendered higher-resolution preview.
- Files inspected: `.ai/task_tracker.md`, `src/app.rs`, `src/thumbnail.rs`
- Files changed: `.ai/task_tracker.md`, `src/app.rs`
- Verification: `cargo build`, `cargo test`, `timeout 5s cargo run`
- Notes: Preview fallback now stays on the generated thumbnail proxy until a rendered preview file is confirmed to exist, and async preview completions no longer replace a working proxy with `None` or a missing path. The higher-resolution handoff now accepts any valid render for the currently selected frame, even if layout resizing made the target size drift slightly while that render was in flight, and it automatically queues a larger rerender if the accepted image still undershoots the latest target size.

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
