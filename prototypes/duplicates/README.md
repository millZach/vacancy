# Duplicate finder · throwaway prototype

Question: where does background hashing show, and how does the user review duplicate groups, for the "Duplicate files" proposal (issue #10)?

Run `npm run prototype:dups` and open http://127.0.0.1:1436/?variant=A. Use the floating arrows or keyboard arrows for A/B/C, Windows/macOS for platform paths, and the circular arrow to start over (hash again). Add `&hash=12000` for slower hashing and `&run=8000` for a slower run. The switcher shows the live state.

Every variant runs the whole flow: scan done with hashing in the background (the map and plan stay usable), proposal ready (Review risk, off by default), review groups and untick one, confirm, simulated run, recap.

- **A · Plan row (Recommended).** Progress is inline in a "Duplicate files" row of the plan tray ("Checking 1,204 files… 38%"). The row expands into the group list, by Photos, Videos and Other files, each group showing what it keeps, what it deletes and why, with a tick box. Recommended because it adds one row to the plan tray Zach already approved and matches his "Vacancy picks, you untick" decision with no new surface.
- **B · Map links.** Progress is a quiet pill in the map toolbar. When ready, the copies are dashed tiles on the map, and a thumbnail panel replaces the tray. Hovering or picking a group draws links from the folder that keeps the copy to the tiles its extra copies are deleted from.
- **C · Compare sheet.** Progress is a floating HUD over the map, which turns into "Compare copies". A full sheet pages through groups one at a time, the kept copy on the left and the deleted copies on the right, with Skip group, a strip of every group and a running count.

The keeper rule is the same everywhere: the tidiest copy (not in Downloads, not named "copy", " - Copy" or "(1)"), and the oldest among the tidiest. Each group says why ("Kept: oldest, in Pictures", "Kept: in Videos, not Downloads", "Kept: original name, not ‘(1)’"). There is no per-group keeper picker. The confirm dialog says that only the extra copies go and one copy of each group stays, and names the groups you unticked.

All data is synthetic and in memory: no bridge calls, no file access. Thumbnails are generated gradients, not images. The prototype imports the real Shell, MapView/overviewModel, Legend, the Checkbox/RiskChip/SourceTag row controls, the RunPanel and run reducer, SummaryMap, HeroNumber and the design tokens, plus run, summary and HUD styles. The confirm dialog and the recap panel reuse the app's markup and classes but are rebuilt here, because the real ones have no place for a "one copy stays" line or a duplicates section. No production source file is changed.

`npm run prototype:dups:check` typechecks, and `npm run prototype:dups:build` builds this route. `node prototypes/duplicates/proof.mjs` serves the build on a temporary preview (port 1437), drives every variant through the flow in Edge at 1280×820 on Windows and 1100×700 on macOS, saves screenshots to `D:\tmp\vacancy-proto-dups\shots\` and fails on page errors. Run build, check and proof through the shared `Invoke-HeavyJob.ps1` wrapper.

The build uses relative asset paths (`base: './'`). Append `?review=1&variant=A` to show the switcher in a build. Without that flag the switcher stays hidden in builds; dev always shows it.

No verdict yet. Don't promote this prototype until Zach picks a design.
