# Agent Workflow

This is the repo-local entrypoint for AI-assisted development in AWEBPinator.

## Start Here

Before editing anything:

1. Read this file.
2. Read `.ai/task_tracker.md`.
3. Inspect the files relevant to the requested change.
4. Check `git status --short` and do not overwrite unrelated work.

Use the rest of `.ai/` as needed:

- `.ai/repo_overview.md` explains what this project is and where the main pieces live.
- `.ai/architecture.md` describes the current Rust/GTK module boundaries and data flow.
- `.ai/workflows.md` lists build, test, run, and verification expectations.
- `.ai/task_tracker.md` tracks active and completed work.

## Required Agent Behavior

- Always inspect relevant files before editing. Do not patch from memory or assumptions.
- Always update `.ai/task_tracker.md` during work. Add a task before implementation, then update status and verification notes before finishing.
- Prefer small, testable changes. Keep UI, export, and data-model changes scoped unless the user explicitly asks for a broader refactor.
- Respect existing patterns and conventions in `src/`, `README.md`, `AGENTS.md`, and `.vscode/`.
- Document uncertainty instead of guessing. If behavior depends on GTK display availability, local tools, or system binaries, say what was and was not verified.
- Preserve unrelated user changes. This repo may have a dirty worktree; never revert or overwrite changes you did not make unless explicitly instructed.

## Safety Rules For This Codebase

- UI-affecting changes need more than `cargo test`; run the app briefly with `cargo run` or `timeout 5s cargo run` when feasible.
- CSS setup must happen after GTK has a display. Do not call `relm4::set_global_css(...)` before GTK app/window initialization.
- Timeline tiles currently show only preview, frame number, and original filename. Selection is a blue background on the full tile, and reorder is by dragging the tile itself.
- Export depends on system `ffmpeg`; diagnostics depend on both `ffmpeg` and `ffprobe`.
- Project save/load uses JSON through `serde_json`; preserve compatibility unless changing the project document format is the explicit task.

## Expected Closeout

End each coding task with:

- What changed.
- What verification ran.
- Any skipped verification and why.
- Any follow-up risks or open questions.
