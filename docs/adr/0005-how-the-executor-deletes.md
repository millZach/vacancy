# 0005: How the executor deletes, and how a run is proven

Date: 2026-10-17. Status: proposed (ticket 04).

ADR 0002 puts safety in the Guard, the fingerprints and re-validation at execution. This records how the executor (ticket 04) carries that out, because a wrong choice here deletes someone's files for good.

## The plan

The planner turns the selected proposals into a plan that can't be changed once made. Each action is one target (a folder deleted whole, a file, or a folder's loose files) with its fingerprint:

- **Identity**: volume serial and file id (Windows), device and inode (macOS), as the scan listed it. The planner walks the target's real chain (never through a link), has the Guard judge it, and checks the identity is still the scan's. A target that fails is left out of the confirm dialog's numbers and reported as skipped.
- **Contents, by identity**: for a folder target, the digest of what the scan listed in it and in every folder inside it, by each folder's identity. A digest is the wrapping sum of a mixed 64-bit key per entry, from its identity (file id; device and inode). The scanner adds up two per folder as it lists: one for every entry, one for files. Two listings with the same sum hold the same entries. For loose files, of which the scan keeps no list, the planner reads the folder. It takes the files the scan didn't keep individually, by name and identity, but only if their digest is the scan's: exactly the files the scan saw. Otherwise it refuses them.
- **Size**: the scan's allocated bytes and file count.
- **Modified time**: the moment the scan began. Nothing in a target may have been modified or changed at or after it (`max(mtime, ctime)`; on Windows the latest of creation, last-write and change times), less a two-second margin for coarse timestamps (FAT, HFS+). Reading never counts as a change: the fixture tests scan, plan and run on volumes with last-access updates on, and nothing reads as changed.

Times alone can't tell that a file is new on Windows, which is why identities are compared. A copy keeps the original's last-write and change times: Explorer and `CopyFile` do, and `robocopy /COPY:DAT` keeps the creation time too. Any program may also set the change time. The pre-review showed a file copied into Temp with its old times passing a time-only check. The digests catch it, because a copy is a new file with a new id. On macOS the kernel always sets the change time, and the digests are a second line.

## Before each action

1. A fresh walk from the volume root, each folder opened by name inside the one before, without following a link, all kept open. Everything after happens relative to these handles; no path is resolved twice.
2. The Guard judges that walked chain (`judge_walked`). Its places are resolved afresh for every step from an `Env` detected for the run. Resolving them for every target would cost about 20 ms each; a plan can hold thousands of targets.
3. The target's identity must be the plan's.
4. A folder is checked whole before anything in it goes. Every folder in it must list exactly what the scan listed there: its digest must be the scan's, and a folder the scan didn't list is new. Every entry must be unchanged since the scan, every folder the one its parent listed (same file id), on the target's volume (and on macOS its mount), no protected place inside it by identity or by name (app bundles only for DerivedData), no volume mounted inside it, and at least the bytes and files the scan measured. On macOS the listing gives link counts, so the upper bound is checked too: files with one name can't add up to more than the scan saw. Windows' listing gives no link counts; there the digests bound what a folder holds. Any mismatch skips the whole target with its reason.

## The delete

- Depth first, relative to open folders. Each folder is listed again and must still match its digest, or nothing in it goes. Each entry is checked again right before it goes.
- **Windows**: the file is opened exclusively (no sharing) with `FILE_OPEN_REPARSE_POINT`. It must have the listed id, carry no link tag, and be unchanged by its handle's own times. Then it is deleted through that handle with `FileDispositionInfoEx`: `POSIX_SEMANTICS | IGNORE_READONLY_ATTRIBUTE`. Where that isn't supported, the read-only attribute is cleared on that file's handle and the classic disposition used, restoring the attribute if the delete fails. A file any app has open fails the exclusive open: a sharing violation, skipped as in use. Folders being deleted are opened without `FILE_SHARE_DELETE`, so nobody can rename or move them meanwhile.
- **macOS**: POSIX has no "unlink this inode". So the entry is checked with `fstatat(AT_SYMLINK_NOFOLLOW)` right before `unlinkat` in its verified folder, and must be the same device and inode, unchanged. A swap in the microseconds between the two calls can only hit a name inside an approved folder, never anything outside it, and never through a link. A file is in use when a process holds it open: every process's open files are read once per step (`proc_pidinfo`, `proc_pidfdinfo`), and a match is skipped with the app's name. A file opened after that read, or only mapped into memory, isn't caught. Locked files (`uchg`) are skipped, never unlocked. A read-only folder inside the target is made writable just before its files go, never the target's parent.
- **Links** inside a target are deleted as links and never followed. A Windows mount point that names a volume (`\??\Volume{...}\`) is not a link: the target is skipped whole, so the volume is never unmounted from its folder.
- **Hard links**: a file counts as freed only when its last name goes. A name removed while another name keeps the file counts as deleted. A note says its space is still in use, kept elsewhere under another name. It isn't a skip, and the step still counts as done.
- **Loose files**: only the files the plan holds by name and identity, each unchanged since the scan. Files new or changed since are left alone. The folder stays.
- **Cancel** is checked before every file, so a run stops after the file being deleted. What is left is reported as cancelled.

## The report

- Per step: what the confirm dialog showed as planned, and freed (allocated) bytes and files deleted. An outcome (`done | partial | skipped`), and skips with reasons, summed by reason per place. A cancel counts what the scan measured of the rest as left.
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
