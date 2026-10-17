# 0005: How the executor deletes, and how a run is proven

Date: 2026-10-17. Status: proposed (ticket 04).

ADR 0002 puts safety in the Guard, the fingerprints and re-validation at execution. This records how the executor (ticket 04) carries that out, because a wrong choice here deletes someone's files for good.

## The plan

The planner turns the selected proposals into a plan that can't be changed once made. Each action is one target (a folder deleted whole, a file, or a folder's loose files) with its fingerprint:

- **Identity**: volume serial and file id (Windows), device and inode (macOS), as the scan listed it. The planner walks the target's real chain (never through a link), has the Guard judge it, and checks the identity is still the scan's. A target that fails is left out of the confirm dialog's numbers and reported as skipped.
- **Size**: the scan's allocated bytes and file count. For loose files, of which the scan keeps no list, the folder's totals of them and the names of the files it kept individually (which are not loose).
- **Modified time**: the moment the scan began. Nothing in a target may have been modified or changed at or after it (`max(mtime, ctime)`; on Windows `max(LastWriteTime, ChangeTime)`), less a two-second margin for coarse timestamps (FAT, HFS+). The change time is what makes this hold: renaming, moving in, hard-linking or setting attributes all set it, and only the OS sets it, so a file moved in with an old modified time still shows as new. Reading never does: the fixture tests scan, plan and run on volumes with last-access updates on, and nothing reads as changed.

## Before each action

1. A fresh walk from the volume root, each folder opened by name inside the one before, without following a link, all kept open. Everything after happens relative to these handles; no path is resolved twice.
2. The Guard judges that walked chain (`judge_walked`). Its places are resolved afresh for every step from an `Env` detected for the run. Resolving them for every target would cost about 20 ms each; a plan can hold thousands of targets.
3. The target's identity must be the plan's.
4. A folder is checked whole before anything in it goes: every entry of every folder unchanged since the scan, every folder the one its parent listed (same file id), on the target's volume (and on macOS its mount), no protected place inside it by identity or by name (app bundles only for DerivedData), no volume mounted inside it, and at least the bytes and files the scan measured. On macOS the listing gives link counts, so the upper bound is checked too: files with one name can't add up to more than the scan saw. Windows' listing gives no link counts. Anything added there carries a new change time, which step 4 already refuses, so the upper bound would add nothing. Any mismatch skips the whole target with its reason.

## The delete

- Depth first, relative to open folders. Each entry is checked again right before it goes.
- **Windows**: the file is opened exclusively (no sharing) with `FILE_OPEN_REPARSE_POINT`. It must have the listed id, carry no link tag, and be unchanged by its handle's own times. Then it is deleted through that handle with `FileDispositionInfoEx`: `POSIX_SEMANTICS | IGNORE_READONLY_ATTRIBUTE`. Where that isn't supported, the read-only attribute is cleared on that file's handle and the classic disposition used, restoring the attribute if the delete fails. A file any app has open fails the exclusive open: a sharing violation, skipped as in use. Folders being deleted are opened without `FILE_SHARE_DELETE`, so nobody can rename or move them meanwhile.
- **macOS**: POSIX has no "unlink this inode". So the entry is checked with `fstatat(AT_SYMLINK_NOFOLLOW)` right before `unlinkat` in its verified folder, and must be the same device and inode, unchanged. A swap in the microseconds between the two calls can only hit a name inside an approved folder, never anything outside it, and never through a link. A file is in use when a process holds it open: every process's open files are read once per step (`proc_pidinfo`, `proc_pidfdinfo`), and a match is skipped with the app's name. A file opened after that read, or only mapped into memory, isn't caught. Locked files (`uchg`) are skipped, never unlocked. A read-only folder inside the target is made writable just before its files go, never the target's parent.
- **Links** inside a target are deleted as links and never followed. A Windows mount point that names a volume (`\??\Volume{...}\`) is not a link: the target is skipped whole, so the volume is never unmounted from its folder.
- **Hard links**: a file counts as freed only when its last name goes. A name removed while another name keeps the file is reported as "kept elsewhere under another name", and the step still counts as done.
- **Loose files**: only files unchanged since the scan, not kept individually, and no more of them than the scan saw. Files new or changed since are left alone. The folder stays.
- **Cancel** is checked before every file, so a run stops after the file being deleted. What is left is reported as cancelled.

## The report

- Per step: planned vs freed (allocated) bytes, files, an outcome (`done | partial | skipped`), and skips with reasons, summed by reason per place.
- Totals: freed by category, the biggest win, duration.
- The drive's own free-space change before and after, shown beside the freed total, unadjusted: other apps write too.
- Each run is saved as JSON in the app data folder's `runs`.

## Proving it: fixture mode

The installed app can run against a fixture profile instead of the real drives: `VACANCY_FIXTURE_ROOT=<folder>`.

- The drive list holds one pseudo-drive, the folder. Home, app data and temp are laid out inside it. The Guard additionally denies, by identity, everything not strictly inside it (`Env::confined_to`).
- The mode refuses a folder that is missing, is reached through a link, is a drive's root, or is in or holds the real home folder. The app then scans and deletes nothing, and says why. It never falls back to the real drives.
- The pseudo-drive's figures are the fixture's own: used is what a scan of it counts, and capacity is fixed at start.
- `cargo run -p vacancy-core --example fixture -- <empty folder>` writes the profile the tests use, and `--list` writes a hashed listing for before-and-after diffs.

## Consequences

- An app writing to its cache makes that cache's target skipped whole as changed, not partly deleted. Running apps' caches mostly survive a run, by design.
- NTFS keeps a copy of a file's times in each folder that lists one of its names, refreshed only through that name. So a hard-linked file can list as new under one name, and the scanner may see a project as recently used (pnpm's store links), or the executor may skip a target as changed. Both err towards keeping.
- On Windows, volumes whose listings have no file ids (FAT, exFAT) are never deleted from: nothing there can be confirmed. The planner already refuses their targets, since the scan recorded no identity for them.
