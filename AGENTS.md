# Vacancy

Vacancy is a desktop app for macOS and Windows that shows where your disk space goes, proposes how to get it back, runs the plan you approve, and ends with a visual report of what it did. Its proposals also cover reorganizing messy folders.

## What is expensive to get wrong

Vacancy deletes files permanently. A wrong rule or a bad path join destroys someone's data, and nothing can undo it.

- **Deletion is permanent, with one confirmation.** Only items listed on the confirm screen are touched, and the executor re-checks each path against the approved plan before acting.
- **Folder moves are journaled.** Every reorganization move is written to a journal before it happens, and the summary offers Undo.
- **Never follow symlinks or junctions while scanning or deleting.** On Windows, a junction can point a "cache" folder at a user's documents.
- **System and protected paths are off-limits.** The rules engine has a deny-list (OS folders, app bundles, the user's home root itself, cloud-sync roots) that no proposal can override, including AI proposals.
- **AI proposes, never executes.** Reorganization ideas from the local Claude Code or Codex CLI become plan items that the user reviews. The model never runs commands or touches files.
- **Both platforms, every time.** Paths, permissions (macOS Full Disk Access, Windows long paths and ACLs) and well-known cache locations differ. A change that touches one platform is tested on both.

## Product decisions

- Stack: Tauri 2. The Rust backend handles scanning, rules, the executor and the journal; the UI is React + TypeScript.
- Proposals: built-in rules find known junk offline (caches, temp, installers, dev build folders, duplicates, Docker/Xcode/package-manager leftovers). When the Claude Code CLI (`claude`) or the Codex CLI (`codex`) is installed, Vacancy asks it to propose folder reorganization and to explain unknown large folders. No API key.
- Deletion: permanent, one confirmation. Moves stay undoable.

## Read first

- This file, then `CONTEXT.md` and `docs/adr/` once they exist.
- `prototypes/` holds the throwaway UI prototypes used to pick the design.

## Run, test, prove

The app is not scaffolded yet. When it is, record the real commands here:

- Run: `npm run tauri dev`
- Test: `cargo test --manifest-path src-tauri/Cargo.toml` and `npm test`
- Build installers: `npm run tauri build` (Windows `.msi`/`.exe`, macOS `.dmg`)
- Prove a change: install the built installer, scan a fixture tree (never a real home folder during tests), approve a plan, and check the summary against what changed on disk.

## CI

No CI yet. The first test adds `.github/workflows/ci.yml`, running on `windows-latest` and `macos-latest`. Its job names become the merge bar here.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on `millZach/vacancy`, used through the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The default five labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` plus `docs/adr/` at the repo root. See `docs/agents/domain.md`.
