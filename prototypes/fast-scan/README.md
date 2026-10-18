# Fast scan (asks for admin) · throwaway prototype

Question: where does Windows' "Fast scan (asks for admin)" live on the scan screen, and how do the prompt, the fast scan, the Locked folders block and the two fallbacks read?

Zach's decisions (2026-10-18) are built in. Windows only. A normal first scan of a big drive takes about 3 minutes; fast scan reads the NTFS file index (the MFT) directly, in 5 to 15 seconds. It's offered per scan. Every fast scan shows Windows' UAC prompt, and nothing is remembered. Admin is used only to read the drive's file index; deleting still happens normally, with the same checks. Folders the account can't normally open show as totals only, in a "Locked folders" block with no names inside, and nothing in them can be proposed or deleted. If the prompt is declined, or the drive isn't NTFS (the exFAT USB stick here), the normal scan runs, with a short note saying why. macOS shows today's scan screen unchanged.

## Run it

- Dev: `npm run prototype:fast-scan`, then open http://127.0.0.1:1437/?variant=A (or `B`, `C`; add `&platform=macos`).
- Build: `npm run prototype:fast-scan:build`, then `node node_modules/vite/bin/vite.js preview --config prototypes/fast-scan/vite.config.ts` and open http://127.0.0.1:1437/?review=1&variant=A. The build uses relative asset paths (`base: './'`), so `dist` also works from any static path.
- Check: `npm run prototype:fast-scan:check` runs TypeScript. `prototypes/fast-scan/check.ps1` runs the typecheck, the build and `proof.mjs`. Run it under `Invoke-HeavyJob.ps1`.

The floating THROWAWAY switcher has ←/→ (the keyboard arrows work too), the variant name, Windows/macOS, Finish scan (while one runs), Start over and a live state readout. It always shows in dev. In a build it shows only with `?review=1`. While it shows, it reserves a 52 px strip at the bottom.

Scans run in prototype time: a normal scan takes 14 s and a fast one about 3 s, so the flow can be walked. The HUD never shows a clock, and the copy uses the real estimates.

## The flow every variant walks

Scan screen (three drives: C: and D: are NTFS, the SanDisk E: stick is exFAT) → choose fast scan → the prompt stand-in (Yes / No) → the fast scan's HUD → the map with Locked folders (hover a tile, click the block to open it) → Scan again. Then the two fallbacks: answer No, and the normal scan runs with a note; pick SanDisk (E:) and ask for a fast scan, and the normal scan runs with a note, with no prompt.

The prompt is a clearly labelled stand-in. Windows draws the real one on its dimmed secure desktop; Vacancy can't style it, word it or skip it. Its question is Windows' own: "Do you want to allow this app to make changes to your device?" That's why every offer says, before the prompt, what admin is used for.

## Variants

- **A · Second button on the plate (Recommended).** Beside "Scan Local Disk (C:)" sits "Fast scan (asks for admin)", with one line under both: how long each takes on this drive and what admin is used for. One click starts either scan. The declined and exFAT notes sit under the HUD's bar.
- **B · Scan mode in the drive picker.** A Normal scan | Fast scan switch sits over the drive cards, and each card lists both times for that drive ("Fast: about 10 s"; SanDisk says "not on exFAT"). The one button follows the mode ("Fast scan Local Disk (C:)"). Choosing Fast shows a note about admin, or, for the stick, that it gets the normal scan. The switch goes back to Normal every time the scan screen comes back, so nothing is remembered.
- **C · Switch from the scan HUD.** The plate is today's, untouched (the real ScanPlate). Scan starts as usual, and the HUD offers "Switch to fast scan" from the first moment, with the times and the admin line. The normal scan keeps going behind the prompt. Yes starts over fast; No collapses the offer to "The normal scan carries on". On the stick the HUD says why there's no offer.

**Recommendation: A.** It's the decision as written: the offer sits next to Scan, and one click asks for exactly one fast scan. The admin line is read before the prompt appears, which matters because Windows' own wording says "make changes". B's per-drive times are worth stealing into A's line (A already gives the drive on screen's times), but B's mode switch is a setting to read before you can scan, and a setting looks like something that will be remembered. C is the gentlest on the plate, but every fast scan costs a normal scan's start first, its prompt lands over a scan already running, and it's close to the "only when slow" offer Zach turned down.

The Locked folders block is the same in every variant. It reads naturally in one place: on the map, as its own block.

## What the map shows

- **Fast scan.** Every used byte is counted. The "Space Vacancy can't see" tile is gone. The file index's own records (1.3 GB) join Windows & system, and a hatched, colourless **Locked folders** block (25.7 GB) holds two totals: "Other accounts on this PC" and "Folders only Windows opens". Its tiles have no names, paths, dates or proposals; clicking one only opens the block. The hover card gives the total, the folder count and why it's locked. Clicking the block opens it with a card: no names, nothing to propose, nothing to delete, and what to do about another account's files (sign in as that account). The toolbar says "Fast scan, 9 s" and "26 GB in locked folders, totals only".
- **Normal scan** (including both fallbacks): today's map. The 27 GB that a normal scan can't see sits in Windows & system as "Space Vacancy can't see", whose hover card now says a fast scan can count it as totals only. The toolbar says "111 folders couldn't be read" and, after a fallback, "Normal scan: admin wasn't given" or "Normal scan: exFAT drive".

All data is synthetic and in memory. The prototype imports the real Shell, DriveMenu, AdvisorTrigger and AdvisorNote, ScanPlate (C and macOS), ScanHud (macOS), DotField, MapView with its model, Legend, the drive bits, RiskChip and SourceTag, and the design tokens and CSS. A and B's plates and every Windows HUD are drawn here with the app's classes. A refusing bridge sits under the components; only `platform` is read from it, and the proof fails on any call. Nothing reads a disk, asks for admin or runs a helper. No production source file is changed.

## Not real yet, or faked here

- There is no elevated helper. The safety direction (to be designed) is a small read-only helper that runs elevated, reads the volume's MFT and never deletes or moves. Its name, path and arguments in the prompt's details are made up.
- The prompt's publisher line only reads "Verified publisher: Vacancy" if the helper is code-signed. Unsigned, Windows shows a yellow "Unknown publisher" prompt, which would undercut every line of copy here.
- Declining the prompt, cancelling it, and the helper failing to start all need telling apart in the real app (the prototype has only No). So do a drive that's NTFS but whose index can't be read (BitLocker locked, a dirty volume).
- The file system per drive is made up here. `VolumeInfo` has no file system field yet, and the picker would need one to say "not on exFAT" before a scan (B) or to skip the prompt (all variants).
- The Locked folders totals assume the core can tell which folders the account can't open while reading the MFT, without listing their names, and that it can split them into the two reasons shown. If it can only give one total, the block has one tile.
- The fast scan's map preview (index read first, then categories appearing) is a guess at how the core would report progress.
- The times are fixed per drive here. Real estimates would need a measure of the drive (used space, file count from a last scan, SSD or HDD).
- With a fast scan, Windows & system includes the file index's records. The current category rules may sort them elsewhere.
- Seen while proving this, and the app's own behaviour, not the prototype's: early in a scan a tiny category ("Other" on the stick) gets a 2 px tall strip, and MapView still draws its header across the empty map, because it checks only the width (`rect.w > 40`).

No user verdict yet. Don't promote this prototype or tag it until Zach chooses.
