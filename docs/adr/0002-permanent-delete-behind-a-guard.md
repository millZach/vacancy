# 0002: Deletion is permanent, behind a Guard and fingerprints

Date: 2026-10-16. Status: accepted (Zach).

Zach chose permanent deletion with one confirmation over sending items to the Trash or Recycle Bin, because it frees the space immediately. Folder reorganization moves stay undoable through a journal.

Because nothing can be recovered, safety lives in three places:

1. **The Guard**: a deny-list (OS folders, app bundles, the home root itself, cloud-sync roots, the Photos library, Time Machine, anything outside allowed roots). It is applied to every proposal, from rules or from the AI, and no setting or proposal can override it.
2. **Fingerprints**: each planned action records its target's size, modified time and file id at scan time.
3. **Re-validation at execution**: before each action, the executor checks the Guard and the fingerprint again, and skips anything that changed, became a link, or is in use. Links (symlinks, junctions, reparse points) are never followed while scanning or deleting.

Any change to these three is high risk and is tested on both platforms.
