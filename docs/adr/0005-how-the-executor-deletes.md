# 0005: How the executor deletes, and how a run is proven

Date: 2026-10-17. Status: proposed (ticket 04).

ADR 0002 puts safety in the Guard, the fingerprints and re-validation at execution. This records how the executor (ticket 04) carries that out, because a wrong choice here deletes someone's files for good.

## The plan

The planner turns the selected proposals into a plan that can't be changed once made. Each action is one target (a folder deleted whole, a file, or a folder's loose files) with its fingerprint:

- **Identity**: the volume and the file, as the scan listed them. On Windows that is the scan's volume serial with the file id; on macOS, the device and inode.
  - The scan keeps each root's volume serial, so another disk at the same drive letter, whose file ids could repeat, never matches.
  - The planner walks the target's real chain (never through a link), has the Guard judge it, and checks the whole identity is still the scan's.
  - A target that fails is left out of the confirm dialog's numbers and reported as skipped.
- **Contents, by identity and size**: for a folder target, a digest of what the scan listed in it and in every folder inside it, keyed by each folder's identity.
  - A digest is the wrapping sum of a mixed 64-bit key per entry, made from its identity (file id; device and inode). A file's key also mixes in its logical and allocated sizes.
  - The scanner adds up two per folder as it lists: one for every entry, one for files.
  - Two listings with the same sum hold the same entries, at the same sizes.
- **Loose files**, of which the scan keeps no list. The planner reads the folder and takes the files the scan didn't keep individually, by name, identity and sizes, but only if their digest is the scan's: exactly the files the scan saw. Otherwise it refuses them.
- **A kept file**: its allocated and logical sizes.
- **Modified time**: the moment the scan began. Nothing in a target may have been modified or changed at or after it, less a two-second margin for coarse timestamps (FAT, HFS+).
  - On macOS that is `max(mtime, ctime)`; on Windows the latest of the creation, last-write and change times.
  - Reading never counts as a change: the fixture tests scan, plan and run on volumes with last-access updates on, and nothing reads as changed.

**Times can't tell a changed file on Windows; identities and sizes can.**
- A copy keeps the original's last-write and change times. Explorer and `CopyFile` do, and `robocopy /COPY:DAT` keeps the creation time too.
- A program allowed to write a file's attributes can set all four times, the change time included (`SetFileInformationByHandle(FileBasicInfo)`), with no privilege beyond that. So times are a first check, never the proof.
- A file copied in has a new id; a file rewritten to another size has other sizes. Both change the digests.
- What no check here can see: a file rewritten in place to exactly the same length and allocation, with all four times set back. Only a hash of every file's contents would, which would mean reading every byte before deleting it. Vacancy doesn't. That is the remaining limit.
- On macOS the kernel always sets the change time, so there the digests are a second line.

## Before each action

1. **A fresh walk** from the volume root, each folder opened by name inside the one before, without following a link, all kept open. On Windows each is held without `FILE_SHARE_DELETE`, so no folder on the way can be renamed or moved while the action runs. Everything after happens relative to these handles; no path is resolved twice.
2. **The Guard** judges that walked chain (`judge_walked`). Its places are resolved afresh for every step from an `Env` detected for the run. Resolving them for every target would cost about 20 ms each, and a plan can hold thousands of targets.
3. **The target's whole identity** must be the plan's.
4. **A folder target is checked whole** before anything in it goes. Any mismatch skips the whole target, with its reason. It checks:
   - every folder in it lists exactly what the scan listed there: its digest is the scan's, and a folder the scan didn't list is new;
   - every entry is unchanged since the scan;
   - every folder is the one its parent listed (same file id), on the target's volume (and on macOS its mount);
   - nothing protected is inside, by identity or by name (app bundles only for DerivedData);
   - no volume is mounted inside;
   - it holds at least the bytes and files the scan measured. On macOS, files with one name can't add up to more either.

## The delete

- **Depth first, relative to open folders.**
  - Each folder is listed again and must still match its digest, or nothing in it goes.
  - Each folder must still be where the Guard judged it. Its identity chain from the volume root is walked again at the start of its batch and before the folder itself is removed. On macOS it is also walked again before every unlink; on Windows, every 256 entries.
  - A folder found anywhere else (renamed into a cloud-sync folder, say) stops the target: the rest stays where it went, and the report says so.
  - **What this guarantees.** On Windows nothing on the chain can move at all: the pinned handles forbid it, and NTFS also refuses to rename a folder with handles open inside it. On macOS a folder can be moved at any moment, so the guarantee is the walk before each unlink. The one window is between that walk and the unlink, the width of a system call; a file being deleted when its folder moves can still go. The test `a_folder_moved_into_a_cloud_folder_mid_run_is_left_there` covers both platforms.
- **Windows:**
  - The file is opened exclusively (no sharing) with `FILE_OPEN_REPARSE_POINT`.
  - It must have the listed id, carry no link tag, be unchanged by its handle's own times, and have the listed length. The length and link count come from one query by handle; if it fails, the file stays, since its size can't be confirmed and its space couldn't be counted honestly.
  - It is deleted through that handle with `FileDispositionInfoEx`: `POSIX_SEMANTICS | IGNORE_READONLY_ATTRIBUTE`. Where that isn't supported, the read-only attribute is cleared on that file's handle and the classic disposition used, restoring the attribute if the delete fails.
  - A file any app has open fails the exclusive open: a sharing violation, skipped as in use.
  - Folders being deleted are opened without `FILE_SHARE_DELETE`.
- **macOS:**
  - POSIX has no "unlink this inode". So the entry is checked with `fstatat(AT_SYMLINK_NOFOLLOW)` right before `unlinkat` in its verified folder. It must have the same device and inode, the same sizes, and be unchanged.
  - **In use**: every process's open files (`proc_pidinfo`, `proc_pidfdinfo`) are read again at the start of each folder's batch and every 256 entries, at most every 250 ms (a read takes tens of milliseconds). A match is skipped with the app's name.
  - **Opened after that read**: at the next read, and when the action ends, Vacancy looks again for the files it deleted. One an app still holds is reported as deleted while held open: its space comes back when the app lets go, so it isn't counted as freed. A file only mapped into memory, with no descriptor, isn't seen.
  - A folder whose mount can't be read (`fstatfs` fails) fails the walk: an unknown mount is never taken as the right one.
  - Locked files (`uchg`) are skipped, never unlocked. A read-only folder inside the target is made writable just before its files go, never the target's parent.
- **Links** inside a target are deleted as links and never followed. A Windows mount point that names a volume (`\??\Volume{...}\`) is not a link: the target is skipped whole, so the volume is never unmounted from its folder. Network drives and volumes without file ids are never deleted from.
- **Hard links**: a file counts as freed only when its last name goes. A name removed while another name keeps the file counts as deleted, with a note that its space is still in use. Notes (this one, and "held open") aren't skips, and the step still counts as done.
- **Loose files**: only the files the plan holds by name and identity, at the planned sizes, each unchanged since the scan, while their folder is still where the Guard judged it. Files new or changed since are left alone. The folder stays.
- **Cancel** is checked before every file, so a run stops after the file being deleted. What is left is reported as cancelled.

## The report

- Per step:
  - planned, as the confirm dialog showed it;
  - freed (allocated) bytes and files deleted;
  - an outcome (`done | partial | skipped`);
  - skips with reasons, summed by reason per place. A cancel or a moved folder counts what the scan measured of the rest as left.
- Live events. Places skipped whole are sent as decided. Files left inside a place are sent as they happen, the first 50 per place. The report has them all, summed.
- Totals: freed by category, the biggest win, duration.
- The drive's own free-space change before and after, shown beside the freed total, unadjusted: other apps write too.
- Each run is saved as JSON in the app data folder's `runs`.

## Proving it: fixture mode

The installed app can run against a fixture profile instead of the real drives: `VACANCY_FIXTURE_ROOT=<folder>`.

- The drive list holds one pseudo-drive, the folder. Home, app data and temp are laid out inside it. The Guard additionally denies, by identity, everything not strictly inside it (`Env::confined_to`).
- The mode needs the marker file the generator writes, so it only ever runs on a folder that holds nothing real. It refuses a folder that:
  - is missing;
  - is reached through a link;
  - is a drive's root, or the root of a volume mounted in a folder (its device or volume isn't its parent's, or the OS says a volume is mounted there);
  - is in or holds the real home folder.

  It also refuses when the real home can't be found.
- When refused, the app scans and deletes nothing, and says why. It never falls back to the real drives.
- The pseudo-drive's figures are the fixture's own: used is what a scan of it counts, and capacity is fixed at start.
- `cargo run -p vacancy-core --example fixture -- <empty folder>` writes the profile the tests use. `--list` writes a hashed listing for before-and-after diffs, reading files without refreshing their access times.

## Consequences

- An app writing to its cache makes that cache's target skipped whole as changed, not partly deleted. Running apps' caches mostly survive a run, by design.
- NTFS keeps a copy of a file's times in each folder that lists one of its names, refreshed only through that name. So a hard-linked file can list as new under one name. The scanner may then see a project as recently used (pnpm's store links), or the executor may skip a target as changed. Both err towards keeping.
- On Windows, volumes whose listings have no file ids (FAT, exFAT) are never deleted from: nothing there can be confirmed. The planner already refuses their targets, since the scan recorded no identity for them.
- On macOS, re-walking the chain before every unlink costs a few system calls per level per file. A deep `node_modules` of 100,000 files takes some seconds longer to delete. Safety over speed.
