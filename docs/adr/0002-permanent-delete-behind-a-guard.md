# 0002: Deletion is permanent, behind a Guard and fingerprints

Date: 2026-10-16. Status: accepted (Zach).

Zach chose permanent deletion with one confirmation over sending items to the Trash or Recycle Bin, because it frees the space immediately. Folder reorganization moves stay undoable through a journal.

Because nothing can be recovered, safety lives in three places:

1. **The Guard**: a deny-list (OS folders, app bundles, the home root itself, cloud-sync roots, the Photos library, Time Machine, anything outside allowed roots). It is applied to every proposal, from rules or from the AI, and no setting or proposal can override it.
2. **Fingerprints**: each planned action records its target's size, modified time and file id at scan time.
3. **Re-validation at execution**: before each action, the executor checks the Guard and the fingerprint again, and skips anything that changed, became a link, or is in use. Links (symlinks, junctions, reparse points) are never followed while scanning or deleting.

Any change to these three is high risk and is tested on both platforms.

**The Guard authorizes by file-system identity, not by spelling** (2026-10-17, after the lead's second re-check of ticket 03). Folded strings kept aliasing distinct folders: NTFS keeps `İpek` and `i` + U+0307 + `pek` apart though they lowercase alike, and a case-sensitive Windows folder keeps `alex` and `Alex` apart. So the allowed roots and the protected places are resolved once per scan to file identities (Windows: volume serial and 128-bit file id from `FileIdInfo`; macOS: device and inode), together with every folder that holds them. Each target is judged by its real chain: its volume root and each folder below it, opened by name inside the one before, never through a link. A target is allowed only if a folder strictly above it is an allowed root, and denied if any folder on its chain is a protected place or the target itself holds one. Name keys that fold case and Unicode form remain, but only ever to deny more (a path spelled like a protected place) and to find candidates for the rules, which confirm each configured place by identity before using it.
