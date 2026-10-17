# 0004: What the scanner counts, and what it never follows

Date: 2026-10-16. Status: accepted (Zach, 2026-10-17).

The scanner reads a whole volume without admin rights, fast enough for a drive of several million files, and must never wander somewhere it shouldn't: a "cache" folder that is really a junction to someone's Documents is how a cleaner destroys data (AGENTS.md, ADR 0002).

## Links

- **Never followed.** Symlinks, junctions, volume mount points and every other reparse point that names somewhere else (Windows' *name surrogate* tags, plus any tag Vacancy doesn't know) are recorded and skipped. The decision is made from the parent's listing, before descending. Folders are opened with `FILE_FLAG_OPEN_REPARSE_POINT` (Windows) or `O_NOFOLLOW` (macOS), so a folder swapped for a link mid-scan opens as the link itself and lists empty. Those flags only guard the last step of the path, so every folder opened must also be the folder its parent listed: its file id and volume serial (Windows) or device and inode (macOS) are compared right after opening. If a folder further up was swapped for a link after being listed, the path leads elsewhere, the identity differs, and the folder is counted as changed and not entered. The check costs one query per folder on Windows and nothing extra on macOS.
- **Cloud-sync placeholders are entered** (Zach's decision, 2026-10-17). OneDrive and Dropbox mark their sync roots (and on-demand folders) with the cloud-files reparse tag (`IO_REPARSE_TAG_CLOUD_*`). These are not links: they name nowhere else, and their contents are local placeholders. The scanner lists them like any folder and counts only the bytes actually on disk (an online-only file has nothing allocated). Skipping them would hide every locally kept synced file; on the laptop this was built on, that is 195 GB of Dropbox. The categorizer puts everything under a cloud root in the protected Cloud category, and deleting from cloud-sync roots stays blocked by the Guard (ADR 0002).
- **Never another file system.** On Windows only reparse points lead to other volumes, so the rule above covers it. On macOS every opened folder's device and mount point (`f_mntonname`) are compared with its root's: the system and Data volumes of the startup disk report the same device number, so the device alone can't tell them apart (seen on the macOS CI runner). The startup disk is walked as its two APFS volumes, each from its own root (`/` and `/System/Volumes/Data`), never through the firmlinks between them, and the Data volume is shown at `/` the way Finder shows it.

## Sizes

- **Allocated, not logical**, from the listing itself: it already reflects NTFS compression, sparse files and cloud placeholders. One case lists wrong: files compressed by the Windows Overlay Filter (CompactOS, `compact /exe`) list with nothing allocated, so files with a length, no allocation and none of the sparse, compressed or cloud attributes are asked again through a handle opened for attributes only. That file is opened relative to the already verified folder handle (`NtOpenFile` with `RootDirectory`), never by path, so a folder swapped for a junction after it was opened can't send the query to a file elsewhere.
- **Hard links once.** On NTFS a file id is a record number in a dense range, so "seen" is one bit per record (half a megabyte for four million files). On macOS the listing gives the link count, and only multi-link files go in a set.
- **The OS's used figure is the truth.** The map shows the scan reconciled with it: used space the scan couldn't count (folders it may not open, restore points, swap, snapshots, file-system metadata) is one item in System, "Space Vacancy can't see", with the reason in its hover card. Nothing is invented to fill the gap.

## Consequences

- A folder the user can't open is counted as unreadable and flagged; its size is part of the hidden item.
- Opening each folder is the cost floor without admin rights. A first scan of a large system drive waits on the disk (3 minutes for 4.5 million files on the build laptop); later scans take 12 to 45 seconds while the OS still has the folders in memory.
