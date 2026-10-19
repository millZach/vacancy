# 0006: Duplicate files, and the copy that stays

Date: 2026-10-18. Status: accepted (Zach, ticket 09). UI: prototype `prototype/duplicates`, variant B "Map links".

Duplicate files are the first proposal that deletes the user's own files rather than caches. A wrong group deletes the last copy of a photo, so the finder is strict about what it calls a copy, and the executor checks the copy that stays before each extra copy goes.

## What counts

- **Scope.** Files of 1 MiB or more in Pictures, Videos (Movies on macOS, and a Videos folder there if one exists), Documents, Downloads and the Desktop. All file types; the proposal counts photos and videos first ("3 photos, 1 video and 2 other files").
- **Only files the scan kept one by one.** The scan keeps a file individually when it takes 1 MiB or more on disk, so a file whose length is 1 MiB or more but which NTFS compression, sparseness or a cloud placeholder keeps under that on disk is never a candidate. The finder then compares lengths, so a candidate is always at least 1 MiB long.
- **Never looked at.**
  - Anything the Guard protects, or a locked category (the OS, a Photos library, cloud-sync folders).
  - A cloud-sync folder, by flag, category or name (OneDrive, iCloud Drive, Dropbox, Google Drive, Box), so nothing disappears from other devices.
  - The inside of a macOS package or library (`.photoslibrary`, `.app`, `.imovielibrary`...), a folder starting with a dot, and developer folders (a Git repository, `node_modules`).
  - A file another delete proposal takes, or one inside a folder it takes. No byte is counted twice, and no proposal can delete a copy this one keeps.
  - Links. The scanner never entered one, and every read is opened through a chain that follows none (`fsid::open_scanned`).
- **Exact copies only.** Grouped by length (the scan's figure), then by a SHA-256 of the first 64 KiB, then by a SHA-256 of the whole file. Most files are never read.
- **Each file read must be the file the scan listed:**
  - the same identity, at the same length, with one name only;
  - unchanged since the scan began (the latest of its modified, change and, on Windows, creation times), and unchanged while it was read.
- **Hard links are one file under two names, not copies.** Deleting a name frees nothing, so a file with more than one name is left out.
- **Clones are left out too, where the platform says so.** On macOS a file whose private size (`ATTR_CMNEXT_PRIVATESIZE`) is less than its allocation shares its data with another file, as Finder's Duplicate makes on APFS: deleting it frees next to nothing, so it is left out like a hard link. ReFS block clones on Windows can't be told, so the proposal's figure is what the extra copies take up, and the copy says up to that much comes back.
- **Nothing is downloaded or recalled.** A file not all on this disk is never read: on Windows one marked offline, recalled on open or recalled on data access; on macOS one marked dataless (`SF_DATALESS`). Each is counted apart, not as changed. Files the Windows Overlay Filter compresses and files Data Deduplication keeps are read like any other; links and cloud placeholders never are.
- **Contents are read for hashing only.** Nothing about them is kept, shown or sent. The panel shows file-type icons, names, sizes and folders, never thumbnails. On Windows the read asks to leave the last-access time alone, so hashing never makes a file look just used.

## The keeper rule

In each group Vacancy keeps one copy:

1. **The tidiest.** A copy outside Downloads and not named like a copy ("copy", "- Copy", "Copy of", "(1)", macOS's "copy 2") beats one with either mark, and one mark beats two.
2. **Among the tidiest, the oldest.** That is the earliest of its created and modified times. A copy made later keeps the original's modified time on Windows but not its creation time, so it still loses.
3. **Then the shorter path, then the path's order**, so the choice never changes between runs.

Each group says why in a few words ("Kept: oldest, in Pictures", "Kept: in Videos, not Downloads", "Kept: original name, not '- Copy'"). There is no per-copy picker. The user can untick any group, and an unticked group deletes nothing. The risk is Review, off by default.

## Background, and the map

- **The search runs after the scan, off the UI thread.** The map is already on screen; a pill on its toolbar shows the progress.
- **It holds the scan's lock only to pick candidates.** No file is read under the lock.
- **It stops** at the next read when the drive is scanned again, goes away, or a plan starts. It reads with one 1 MiB buffer, so memory stays flat whatever the files' sizes.
- **When it is done, the proposal is added and the map redrawn.** The categories are carved again, so every byte is still in exactly one tile. The duplicates get one tile per personal folder holding extra copies, in that folder's category. The map can then draw a link from the tile where a group's kept copy lives to each of those tiles.

## Deleting

- **Extra copies are file targets like any other.** They pass the Guard and the fingerprint checks of ADR 0005 when planned and when run. A file changed since the scan is skipped with its reason.
- **Each one carries the copy its group keeps.** Before it goes, the executor finds the kept copy again: a fresh walk that follows no link, the same identity, an ordinary file, the same length and allocation, unchanged since the scan. If it isn't, the extra copy stays, and so does every other copy of that group, with the reason.
- **The kept copy is held while the extra copy is deleted.** On Windows it is opened without `FILE_SHARE_DELETE` from a chain of folders held the same way, so nobody can delete, rename or move it, or a folder above it, until the extra copy is gone. POSIX can't stop that on macOS: there Vacancy holds a descriptor to the kept copy and its folder and looks again, by name in that folder, right before the unlink beside it. The window left is the one ADR 0005 names for every unlink on macOS, the width of a system call.
- **A duplicates target is never planned without its kept copy.** One whose group or kept copy can't be found is refused, with a reason, never planned as a plain delete. A skipped group the scan's duplicates don't have is refused too, and `plan()` refuses the duplicates proposal: it is planned only with the groups to skip. A scan is searched once, so the groups the user unticked are named by the search that found them.
- **The confirm dialog and the summary say one copy of each group stays.** They also say how many groups the user left out, and the summary names a few pairs (the copy kept, a copy deleted).
- **Delete wins over folder tidying** (ticket 08) when both touch a file. When duplicates are found, every extra copy that is a loose file of a sorted folder is marked as taken by the duplicates proposal, by identity, in the sorts' reviews. A plan that still deletes it leaves it out of the sort. An unticked group's extra copies are deleted by nothing, so they are sorted like any other file.
- **Deletes run before sorts.** A kept copy that a sort moves is still at its scanned path when its group's extra copies are checked and deleted. Only then does the sort move it, journaled and undoable like any move. An extra copy skipped by the run (changed since the scan, say) was planned as deleted, so no sort of that plan moves it.
- **A run's checkpoint report carries the duplicates.** The report kept before any file moves already has what the duplicates step did, so a run reopened after the app stopped mid-sort still tells it.

## Limits

- **Same length, same allocation, times set back.** A kept copy rewritten in place this way between the hash and the run is the limit ADR 0005 names. Vacancy doesn't hash every file again before deleting.
- **Same contents only.** Two copies of a photo saved at another quality are different files to the finder. It never guesses at near-duplicates.
