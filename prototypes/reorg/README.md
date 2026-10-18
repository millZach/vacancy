# Folder reorganization · throwaway prototype

Question: where does reorganization live in the Atlas flow, and how do we show before and after?

Zach's decisions (2026-10-18) are built in. Proposals come from built-in sorting rules, offline, with no AI. They sort loose top-level files in Downloads, Desktop and Documents into folders by kind and year, with a "Sort later" folder for files the rule isn't sure about. macOS has no reorganization at all.

## Run it

- Dev: `npm run prototype:reorg`, then open http://127.0.0.1:1435/?variant=A (or `B`, `C`; add `&platform=macos`).
- Build: `npm run prototype:reorg:build`, then `node node_modules/vite/bin/vite.js preview --config prototypes/reorg/vite.config.ts` and open http://127.0.0.1:1435/?review=1&variant=A. The build uses relative asset paths (`base: './'`), so `dist` also works from any static path.
- Check: `npm run prototype:reorg:check` runs TypeScript. `prototypes/reorg/check.ps1` runs the typecheck, the build and `proof.mjs`. Run it under `Invoke-HeavyJob.ps1`.

The floating THROWAWAY switcher has ←/→ (the keyboard arrows work too), the variant name, Windows/macOS and a live state readout. It always shows in dev. In a build it shows only with `?review=1`. While it shows, it reserves a 52 px strip at the bottom so it never covers a footer.

Every variant runs the whole flow: plan → preview → confirm → simulated run → Wrapped's Moved slide with Undo → recap with Undo → Undo result → Past runs with Undo. State is in memory and survives switching variants. Switching platform starts over.

## Variants

- **A · Tidy up in the tray (Recommended).** A "Tidy up" group in today's plan tray, below Delete. Each folder is a row with a tick box and a mini before→after: a pile of loose files, then bars for the folders they go to. Clicking the row opens a side drawer over the map with Before and After trees, sample moves (including a "(1)" rename), what stays and why, and the rules it follows. Undo sits on each folder in the recap, the Moved slide and Past runs.
- **B · Tidy mode on the map.** The map gets a Space | Tidy switch. Tidy shows the three folders as tiles sized by number of files. A scrubber (the summary's Before/After switch plus a track) runs from "As it is" to "Sorted", and the loose-files tile shrinks while its folders grow out of it. A chip on each folder adds it to the plan. Clicking a folder's name zooms in, and the tray shows its sample moves. In the recap, Undo chips sit on the folders, and an undone folder goes back to loose, with the tile that still holds files outlined.
- **C · Review sheet.** The tray has one Tidy up card with a "Review sorting" button. That opens a sheet over the map with a tab per folder and every destination folder listed (`Downloads\Images\2026 · 38 files`), each with its own tick box, sample names and rename notes. The recap is a ledger per destination, with Undo per folder and per-destination results.

**Recommendation: A.** It keeps reorganization in the one plan people already read, with the same tick box, why and footer. The mini preview and the drawer's two trees answer "what will my folder look like" without a new mode. B is the most vivid, but it gives the map a second meaning (file count, not bytes). C gives the most control, but it adds a whole screen of decisions for something that frees nothing.

## What it stays honest about

- Sorts start unticked. Moves never count toward "will be freed", and every total says "Moved, not deleted: frees 0 B". The confirm dialog is the app's own, so it shows the move count and says moves can be undone.
- Nothing is overwritten. A name that's already taken gets " (1)", fixed before review (`Docker Desktop Installer (1).exe`, `fonts (1).zip`, `tax-return (1).pdf`).
- A dropped idea shows its reason (`invoice-0425.pdf` changed after Vacancy looked). Files the rule leaves alone say why (an Office lock file, shortcuts, `desktop.ini`).
- At run time, a file that's open or changed is skipped and reported, on the run panel and in the recap.
- Undo reports what it couldn't restore ("2 files couldn't go back: a file now has the original name"). It removes only the empty folders it made, and says which folders it kept because they still hold files.
- Moves stay on the same drive, inside the folder being sorted. Only loose files at the top level move; subfolders stay. The numbers stay under the engine's 512-entry inventory bound.

All data is synthetic and in memory. The prototype imports the real Shell, MapView/overviewModel, Legend, plan-tray row controls, PlanTray, ConfirmDialog, RunPanel, SummaryMap, Finish (Mac) and the design tokens and CSS. A refusing bridge sits under the components that need one; only `platform` is read from it, and the proof fails on any call. Nothing reads, moves or deletes a file. No production source file is changed.

## Not real yet, or drawn here instead of in the app

- The recap panel, the Moved slide's Undo and Past runs are drawn here with the app's classes. The real SummaryPanel has no Undo, Wrapped's deck can't take a button, and HistoryMenu reads the bridge.
- The real confirm dialog says moves can be undone "from the summary". Undo is now also in Past runs, so production copy should say so.
- On the real run panel a sort step shows "0 B", and a skipped file reads "…, 2 MB kept". That wording is written for deletes and reads oddly for a move.
- "Files in your Delete list aren't moved" is a promise the planner doesn't make yet. "Old installers in Downloads" and "Sort Downloads" can both be in a plan today.
- The core has the journal and Undo engine but no built-in sorting rule that writes drafts. The kinds, years, "Sort later" and the reasons shown here are proposed copy.
- There is no design for an inventory over 512 entries (the engine reports it as truncated).

No user verdict yet. Don't promote this prototype or tag it until Zach chooses.
