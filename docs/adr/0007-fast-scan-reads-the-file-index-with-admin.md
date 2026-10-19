# 0007: Fast scan reads the file index with admin

Date: 2026-10-18. Status: proposed (ticket 11, issue #24).

A normal scan opens every folder as the user (ADR 0004). On this laptop's C: (4.5 million files) a first scan after a restart waits on the disk for 46 to 216 seconds; a warm one takes 10 to 14 (`.zstack/vacancy-v1/scan-tune/results.md`). Reading NTFS's file index, the Master File Table (MFT), takes one long sequential read instead, but needs admin: only administrators may read a volume's raw sectors. Zach chose (2026-10-18, prototype `prototype/fast-scan` variant A) to offer it per scan as "Fast scan (asks for admin)", with Windows' admin prompt every time and nothing remembered; folders the account can't open as totals only; and the normal scan, with a note saying why, whenever the fast scan can't run.

Vacancy deletes files. Admin must never reach the part that deletes, and what admin reads must never be trusted to decide what is deleted.

## What runs with admin

Only `vacancy-index-reader.exe`, a small program bundled beside the app. It:

- takes exactly `--volume X: --pipe <32 hex digits>`, and refuses anything else, or a drive letter that isn't a fixed or removable drive (never a network, optical or RAM drive);
- opens `\\.\X:` for reading (`FILE_READ_DATA`, share read and write), and nothing else;
- reads the boot sector, `$MFT` and `$Secure:$SDS`, and of each file only what its record says: names, sizes, dates, attributes, file ids, parents, reparse tags and security ids. Of a file's data only the sizes in its `$DATA` header are read: never its value, even when it is small enough to sit in the record;
- never creates, writes, moves or deletes a file, never writes to the volume, and talks to nothing but the pipe;
- exits after 30 minutes whatever happens, as soon as the pipe breaks, and as soon as the app that started it ends (it holds that process from its start; see below);
- is ended by the app too, if it hasn't ended five seconds after the app is done with it, whatever way the app was done (stream read, cancelled, never connected).

It loads its DLLs from System32 only: linked with `/DEPENDENTLOADFLAG:0x800` (`LOAD_LIBRARY_SEARCH_SYSTEM32`), calling `SetDefaultDllDirectories` first, and built with the C runtime linked in (`+crt-static`), so it imports only Windows' own DLLs (kernel32, advapi32, ntdll, bcryptprimitives and the API sets), never `VCRUNTIME140.dll`. Its folder may be writable by the user (the per-user install), so a DLL dropped beside it must never load. A build that couldn't promise this fails: a non-MSVC Windows build of the crate stops in `build.rs`, and a release build of the reader without `+crt-static` doesn't compile. It carries a version resource ("Vacancy scan helper"), so Windows' prompt names it. It is unsigned for now (Zach, 2026-10-18): the prompt says "Unknown publisher".

Deleting stays in the app, unelevated, exactly as before: the planner, the Guard and the executor never see admin.

## Starting it, and the pipe

The app:

1. Opens the reader beside its own executable with read access and read sharing only. While that handle is open the file can't be written, renamed or deleted, and Windows refuses to rename a folder holding an open file, so the path can't be redirected either. In a build that bundles the reader, its SHA-256 must be the one recorded at build time (`tools/tauri.mjs` passes it as `VACANCY_READER_SHA256`); a debug build runs the reader built beside it; a release build without a recorded hash refuses.
2. Asks Windows to start it with admin (`ShellExecuteExW`, verb `runas`, in System32, owned by the app's window). `ERROR_CANCELLED` (No, or the prompt closed) is "declined". The handle on the file is released once Windows has started the process from it.
3. Connects to `\\.\pipe\vacancy-index-<token>` as a client with `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`, checks with `GetNamedPipeServerProcessId` that the server is the process Windows started, and writes the fixed 16-byte hello. Nothing else is ever sent to the reader.

The reader (the pipe's server):

1. Creates the pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE` (a name someone made first fails), one instance, `PIPE_REJECT_REMOTE_CLIENTS`, and a DACL granting only read, write and synchronize to its parent process's account, at medium integrity: no right to make further instances.
2. Waits up to 60 seconds for a client, and checks `GetNamedPipeClientProcessId` is its parent process (the app, which `runas` makes its parent). It opens the parent at its start and holds it for its whole life, so the parent's id can't pass to another process while it waits; it refuses a "parent" younger than itself (an id reused before it looked); and it exits when the parent ends.
3. Reads the hello, and nothing more. Then `ImpersonateNamedPipeClient`, `OpenThreadToken` and `RevertToSelf` at once (the reader exits if it can't revert). The token is identification level: enough for `AccessCheck`, never enough to act as the app.

The token is a random 128-bit value from `BCryptGenRandom`, so the name can't be guessed.

### Threat model

UAC is not a security boundary (Microsoft says so); a process running as the user can already ask for admin with a program of its own. The aim is not to make that worse.

- **Another process of the user's swaps the reader** before it starts, to be run with the admin the user meant for Vacancy. The held handle stops a swap during the launch; the pinned hash stops a swap before it (in a release build). A process that can rewrite `vacancy.exe` itself is beyond this, as it is beyond every per-user install. The MSI installs per machine, under Program Files, where users can't write.
- **A DLL planted beside the reader.** System32-only loading.
- **A squatter on the pipe's name** (a process that read the token from the app's memory). The reader's `FIRST_PIPE_INSTANCE` fails, and it exits; the app falls back to the normal scan. A squatter's own pipe fails the app's server-PID check.
- **Another process connecting to the reader first.** The reader refuses any client but its parent, and the DACL admits only the parent's account. Even a client that got through would learn only what its own account can list: see the next section.
- **A malicious pipe server impersonating the reader's admin token.** The reader is the server; a server can't be impersonated by its client. The app allows identification only, so the reader can't act as the app either.
- **A hostile or broken stream** (a reader swapped despite all this, a damaged index). The app reads it as data, never as authority: see "Reading the stream".

## Folders the account can't open

A normal scan can't list a folder its account may not open, and counts it as unreadable (ADR 0004). Fast scan can read it, but Zach chose totals only, with no names.

The reader decides, with the app's own token from the pipe, as a normal scan would meet each folder: it opens each by its path, so a folder can be listed only if the account can also pass through every folder above it (`FILE_TRAVERSE`), unless the token has `SeChangeNotifyPrivilege` ("bypass traverse checking", every account's by default) enabled. A folder whose parent can't be passed through is locked even if its own descriptor would let it be listed. For each folder, the drive's root first, `AccessCheck` of its security descriptor (from `$Secure:$SDS` by the security id in its `$STANDARD_INFORMATION`, or from the record itself on volumes older than NTFS 3.0) for `FILE_LIST_DIRECTORY | SYNCHRONIZE`, what a normal scan opens it with, with file generic mapping. A descriptor that can't be found or isn't valid counts as locked. Verdicts are cached by security id, so a volume with millions of folders needs a few thousand checks.

- A root the account can't list ends the read before any name is sent (the app runs the normal scan, which sees nothing either): bypass-traverse would otherwise let the app reach what it can't list.
- The descriptors come off the disk, and `IsValidSecurityDescriptor`, `GetSecurityDescriptorLength` and `AccessCheck` follow their offsets without a length. So each is first checked in Rust (`vacancy_index::sd`): self-relative, an owner and a group, every SID, ACL and ACE (and each ACE's SID) inside its bytes, only ACE types Windows defines. Then a 4-byte-aligned copy goes to Windows.
- `AccessCheck` failing for any reason but a bad descriptor (a bad token) stops the read: a drive of "locked" folders would be wrong.

A locked folder is sent the way a normal scan sees it: its own name in its parent's listing, flagged locked. Nothing inside it is sent but one frame of totals: its bytes, files and folders. **No name inside a locked folder ever leaves the elevated process.** Files are counted in locked totals only when every name they have is inside locked folders (a file hard-linked into a visible folder is the app's to count there), each once.

In the app, a locked folder is in the snapshot exactly as a normal scan records a folder it may not open (`DirFlags::UNREADABLE`, never entered, no children): no rule can propose it or anything in it, and the planner and executor treat it as before. Its totals go to the map's "Locked folders" block, sorted by path: "Other accounts on this PC" (inside the folder that holds the user's home, but not the home itself) and "Folders only Windows opens" (the rest).

## Reading the index

Vacancy has its own parser, in `crates/vacancy-index`, plain safe Rust over bytes, tested on both platforms.

- `ntfs` (Colin Finck; MIT/Apache-2.0) is careful and read-only, but its last release is 0.4.0 of June 2023, and it reads one record at a time through seeks: no bulk scan of five million records.
- `mft` (omerbenamram; MIT/Apache-2.0, 0.7.0 of December 2025) is a forensic parser of `$MFT` dumps. It would bring some twenty dependencies (`rand`, `lru`, `csv`, `jiff`, `serde`...) into the elevated program, and doesn't merge extension records into their base records.
- `ntfs-reader` (MIT/Apache-2.0, 0.6.0 of October 2026) reads a live MFT, but is Windows-only (no tests on the macOS runner), single-maintainer, and pulls in the `windows` crate.

The parser:

- reads the boot sector (an `-FVE-FS-` or other signature is "not NTFS": a BitLocker-locked volume), then `$MFT`'s own record and its runs, following its attribute list into extension records when the index is fragmented;
- reads every record in 4 MiB sequential chunks on one thread while parsing on another;
- applies each record's update sequence array (a mismatch is a torn record: left out and counted);
- merges each extension record's attributes into its base record by the base reference, once every record is read and only when the base is in use with the sequence the extension names (a stale extension never lends a name to a reused record), so attribute lists of ordinary files need no chasing;
- keeps every `$FILE_NAME` but DOS 8.3 ones (a listing never shows a DOS alias of a long name; a file whose only name is a DOS one, which only other systems make, is left out: an undercount), and links a name into its folder only when the folder's sequence number is the one the name expects (a folder deleted and its record reused orphans the name);
- bounds every read: a field past its record or attribute makes the record malformed, never a panic; offsets and sizes are checked arithmetic, and an index with more names than 32-bit offsets reach is refused;
- a read that fails part way (a bad sector, a drive unplugged) ends the read with an error, sent to the app;
- trusts the boot sector's size only as far as the drive's real length (`IOCTL_DISK_GET_LENGTH_INFO`), makes rows only for records it has read, and reads at most 64 MiB of descriptors kept outside records, so a crafted drive can't make the elevated reader claim memory for an index it doesn't have.

What a listing would say is reproduced, measured against `FileIdExtdDirectoryInfo` on Windows 11:

- allocation of a non-resident stream: its header's allocated size, or for a compressed or sparse stream its real (total) allocation;
- a resident stream: its length rounded up to eight bytes (a 5-byte file lists 8, a 700-byte one 704);
- a file the Windows Overlay Filter compressed (`compact /exe`, CompactOS) lists as a plain file with nothing allocated, and Windows reports its compressed data's allocation by handle, which is what the normal scan asks for. Fast scan gives it the `WofCompressedData` stream's allocation and hides the tag, as the filter does;
- times from `$STANDARD_INFORMATION`, the folder's own attributes with `DIRECTORY`; NTFS's own files (records below 16) and everything under `$Extend` are hidden, as listings hide them.

Their bytes, and every attribute that isn't a file's own data (folder indexes, alternate streams, attribute lists), are "File system records", one item in System.

## Reading the stream

The reader sends the visible folders from the root down, each as `Dir`, `Entries` frames and `DirEnd`, then `Locked` totals, `Records` and `End` (`vacancy_index::wire`). The app (`crates/vacancy-core/src/fast.rs`) fails closed, and runs the normal scan, on anything that doesn't add up:

- a frame longer than 1 MiB, an entry or name past its frame, an unknown frame, a second header;
- a header for another volume (its serial is checked against the app's own handle on the drive) or a drive smaller than the OS says;
- a folder listed that wasn't announced, listed twice, announced twice (a cycle), or not listed by the end; locked totals for a folder that wasn't announced locked;
- a name that is empty, `.`, `..`, or holds `\`, `/` or NUL;
- an entry whose flags don't match what its attributes say (a link marked entered, a file marked a folder);
- a file bigger than the drive, or all files (each once) plus locked totals plus records more than the drive holds;
- more names than four a record, which no real index has;
- a stream that ends early, or a reader silent for two minutes; frames out of their place (progress once folders are listed, records inside a folder, an end that miscounts);
- in fixture mode, a fixture folder the account can't open (no empty map: the normal scan runs).

Entries go through the normal scan's `Collector` and `Builder`: hard links by record number, kept files, markers, and the per-folder digests the executor checks (ADR 0005) are made exactly as for a listing.

## Fallbacks

Each falls back to the normal scan, and the HUD and the map say why:

- the prompt declined or closed: "admin wasn't given";
- the drive isn't NTFS (from `GetVolumeInformationW`; no prompt is shown);
- the reader couldn't start, or didn't connect within 30 seconds, or isn't the one bundled, or the volume has no drive letter to name it by;
- the index couldn't be read: the reader's error (not NTFS on disk, unreadable, the root not listable, checks that couldn't be made), or a stream that doesn't add up.

## Fixture mode

The reader reads the fixture folder's volume. The app keeps only the subtree whose root has the fixture folder's file id (from its own link-free walk to it), and the Guard keeps denying everything outside the fixture by identity, as before.

## Tolerance

A fast scan's map should match a normal scan of the same drive except for:

- files that changed between the two scans;
- locked folders: only fast scan counts them;
- where a file with several names is counted: in the folder of the name a scan meets first. The normal scan's threads race for it; the fast scan meets the shallower name first (a pnpm store before the project linking into it). Totals are the same; a folder's own bytes, and a proposal's, can differ by such files. The executor counts a file freed only when its last name goes, so a run's freed bytes never depend on it;
- what listings get wrong and the index doesn't: a file still open for writing lists its size from the last time its folder entry was updated; the index has its current size. A folder holding such a file has digests that differ from a listing's, so a target containing it is skipped as changed at execution. That errs towards keeping.

`.zstack/vacancy-v1/t24-uac-check.ps1` compares the two on C: and writes the differences.

## Consequences

- Every fast scan shows Windows' admin prompt, by design. Unsigned, it says "Unknown publisher".
- The reader holds the index in memory while it works: about 40 bytes a record, 20 a name and the names themselves, some 400 MB for five million files, for seconds.
- What fast scan sees and a listing doesn't: a cloud placeholder or other reparse point whose reparse data is too big for its record has no tag the reader can read, so it is taken for a link and not entered; and folders a filter (Controlled Folder Access) refuses beyond their ACL are listed. Either way the executor's checks at run time, unelevated, still apply.
- Proposals, the plan and deletion are the same code as after a normal scan. Nothing admin read is trusted to delete: every target is walked, judged and identity-checked again by handle at execution, unelevated.
- A future signed reader changes only the prompt's publisher line, and the hash recorded must then be the signed file's.
