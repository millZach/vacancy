# Live Claude · throwaway prototype

Question: Zach is turning on live AI. What do Claude's jobs look like, and where do they live in the app? The jobs are turning Claude on (and off), a second opinion on the plan, "Let Claude pick", "Why is this risky?" chats, explanations of big folders the rules don't recognise, and Claude's Tidy up layouts. Privacy mode was added during the build.

## Decisions built in

- Live AI uses Zach's own Claude Code CLI only (Codex shows as "Not used"). It's **off until he turns it on**, after a privacy notice: folder and file names, sizes, counts and dates go to Anthropic through his Claude Code. File contents never do while privacy mode is on.
- Claude **proposes only**. Everything it suggests passes the Guard and his review like any rule proposal, and nothing runs until he confirms.
- **Review my plan** flags risky proposals with a short reason. It can't add anything. You can Untick it, Keep it, or ask why.
- **Let Claude pick** is one button, with no goal to type. Claude ticks and unticks the rules' proposals and adds its own items, marked "Suggested by Claude" with its note. Its own items start unticked and as Review. Zach sees the changes against his current plan ("Your plan 75.3 GB → Claude's 63.4 GB", what it unticks, ticks and adds, and what the Guard refused) and then picks Use Claude's plan or Keep mine.
- **Why is this risky?** is a chat about one proposal or folder. Claude's first answer covers what it is, what you'd lose, and whether it comes back. Then there are suggested follow-ups and a box for your own questions.
- **Explain big folders** covers the eight big folders on the map the rules don't recognise. They get a ✳ badge and "Claude says" text. Its suggestions are unticked, "Suggested by Claude", and Guard-checked. The Guard refuses two of them: Windows Update downloads (inside Windows) and WER archives (ProgramData).
- **Claude's Tidy up layouts** sort loose files by what they're for (Money & paperwork, Travel, Work, Photos, Screenshots…) instead of by kind and year, still with Sort later. The moves stay journaled and undoable, and the drawer keeps its before and after trees. Zach picks Built-in rules or Claude's layout per folder.
- **Privacy mode** is on by default when Claude is turned on. With it on, Claude gets names, sizes, counts and dates only. With it off, Claude can read files anywhere on the PC. It still can't write, delete, run commands or go online, and it still only proposes. Turning it off needs a short confirm that says file contents go to Anthropic through Zach's Claude Code, and the choice is remembered: turning Claude off and on again keeps it. Even with privacy mode off, these are **always hidden**:
  - keys and sign-ins: SSH/GPG keys, .aws/.azure/gh/npm/Docker tokens, .env files, Claude's and Codex's own sign-in files, Windows credential and DPAPI folders, the macOS Keychain
  - browser data: saved passwords, cookies, history and autofill (cache folders stay readable)
  - password managers and wallets: 1Password, Bitwarden, KeePass .kdbx files, crypto wallets
  
  "What always stays hidden" folds open in the advisor dialog, in B's panel and in the confirm. Every answer says what it was based on: "From names, sizes and dates only" with a lock, or "I opened Cargo.toml in each project…" with an eye. Answers given with privacy mode off are visibly richer. Example: the Rust row's chat finds `raytracer\target\renders`, 1.2 GB of PNGs cargo won't rebuild, and asking for the review again adds a third flag. Answers that meet a hidden area say so. The Edge profile ("User Data", 3.4 GB on the map) answers "I can't look inside the rest: Login Data, Cookies, History and Web Data hold browser sign-ins and history, which always stay hidden." An answer keeps the privacy mode it was made with. After you change the mode it offers "Ask again with privacy mode off".

## Run it

- Dev: `npm run prototype:ai-advisor`, then open http://127.0.0.1:1438/?variant=A (or `B`, `C`).
- URL params: `&state=off|notfound|signedout|on|slow|error` (the advisor's state; `slow` replies take about 8 s with Cancel, and `error` fails), `&privacy=on|off`, `&delay=ms` (the fake reply delay, 900 by default).
- Build: `npm run prototype:ai-advisor:build`, then `node node_modules/vite/bin/vite.js preview --config prototypes/ai-advisor/vite.config.ts` and open http://127.0.0.1:1438/?review=1&variant=A. The build uses relative asset paths (`base: './'`), so `dist` works from any static path.
- Check: `npm run prototype:ai-advisor:check` runs TypeScript. `prototypes/ai-advisor/check.ps1` runs the typecheck, the build and `proof.mjs` (run it under `Invoke-HeavyJob.ps1`). The proof drives every variant through every job and every state in Edge at 1280×800, plus a 1024×700 pass, and saves about 80 screenshots to `prototypes/ai-advisor/shots/`. It fails on a page error, a bridge call, a page or panel that leaves the window, a clipped toolbar or header, or a missing line.

The floating THROWAWAY switcher has ←/→ (the keyboard arrows work too), the variant name, AI on/off, the advisor state, Privacy on/off, Start over and a live state readout. It always shows in dev. In a build it shows only with `?review=1`.

Everything is in memory and canned. The prototype imports the real Shell, DriveMenu, AdvisorTrigger ("Rules only"), MapView with its model, Legend, RiskChip, SourceTag, Checkbox, the Tidy up model (trees, moves, stays) and the real ConfirmDialog, plus the app's CSS, all on the Windows fixture (`src/bridge/fixtures.ts`). The tray, the Tidy drawer and the advisor dialog (layout C) are drawn here with the app's own classes, because Claude's parts need slots the real ones don't have (see Gaps). A refusing bridge sits under the components, and the proof fails on any call. No production source file is changed.

## Variants

- **A · In the tray (Recommended).** Every job sits on the thing it's about.
  - The header's "Rules only" opens the focused dialog, which now has Turn on Claude, the privacy notice and the hidden areas. When Claude is on, the dialog has a Privacy mode switch beside the Claude Code switch, and Turn off.
  - The tray gets a Claude bar: Review my plan, Let Claude pick, and the privacy chip. Flags appear inline on rows, amber, with Untick it, Keep it and Why?. Rows that weren't flagged get a green "Claude: comes back with cargo build" line.
  - Let Claude pick previews Claude's plan in the tray itself: the same tick boxes in Claude's state, struck-through unticks, Claude's own rows ghosted in, the Guard's refusals in the bar, and the foot's total showing Claude's figure. The foot's button becomes Keep mine / Use Claude's plan.
  - "Ask Claude" on each row, or a click on a ✳ folder, opens a chat drawer in the Tidy drawer's slot over the map.
  - "Explain big folders" in the map toolbar adds a "Claude says" section to hover cards.
  - The Tidy drawer gets a Built-in rules | Claude's layout switch above its trees.
- **B · Claude panel.** One docked panel on the left holds every job as a running conversation.
  - When Claude is off, the panel itself is the turn-on step, with the notice, the hidden areas and Check again. There's no dialog.
  - When Claude is on, its head has the Privacy mode switch and Turn off. Its foot has four action chips (Review my plan, Let Claude pick, Explain big folders, Suggest Tidy up layouts) and a composer with an "About: Docker" chip.
  - Replies are cards in the log. Item names in them are links that light the tile and scroll the tray row into view. The pick's diff, with Use Claude's plan / Keep mine, is a card. Each folder gets a Built-in | Claude's pick card and "See the trees" for the drawer.
  - "Ask Claude" on a row, or a click on a ✳ folder, starts a thread in the panel.
  - The real tray on the right stays as it is.
- **C · Review step.** Claude is its own pass between the plan and the confirm.
  - When Claude is on, the tray's button becomes "Review with Claude", with "Skip the review: free 75.3 GB now" under it.
  - The step replaces the map, with a breadcrumb: Plan › Check with Claude › Confirm. Every item gets an annotation: a flag, a green "why it's fine", or Claude's note on its own items. Ask expands a thread inline under the row.
  - A Your plan | Claude's picks switch shows the pick as markers on the same list, with the Guard's refusals as rows, then Use Claude's picks / Keep mine.
  - The right column compares Built-in and Claude's after-trees for each folder, side by side, and you pick one.
  - On the map, "8 folders the rules don't know" adds ✳ badges. A click opens Claude's card at the bottom left, with "Ask a follow-up".

**Recommendation: A.**
- Claude's work lands where the decision is already made: on the tray row you tick, in the drawer whose trees you already read, and on the hover card you already use. No new place to learn, and the plan stays one plan.
- The pick's diff in A is the plan itself, in Claude's state. That's the most honest way to show what changes before accepting, and the foot's total moves with it.
- A fits at 1024×700. B's three columns leave the map about 384 px wide there.
- Steal from C: its side-by-side Tidy compare for the drawer (A shows one layout at a time), and its "Your plan | Claude's picks" switch as the label for A's preview.
- Steal from B: a short per-session history of what Claude said, and the item links that light a tile.
- Against B: a running conversation buries results (a review, a pick and three chats scroll each other away), and every reply needs links back to the map.
- Against C: a step between plan and confirm makes Claude feel like a required gate. Its "Skip the review" link is the tell. It also takes the map away while you decide.

## Gaps found

- **The confirm dialog and the summary say nothing about Claude.** The real ConfirmDialog is shown with a dashed "Proposed line" injected: "1 item was suggested by Claude: Android emulator images (4.6 GB). It passed the Guard and you ticked it" and "Downloads is sorted into Claude's folders. Moves are journaled, so Undo still works." The end card proposes the summary's version. The prototype stops before the run. `PlanSummary`/`PlanStep` and `RunReport` have no source per step, so neither the dialog nor the Finish nor Past runs can say which items were Claude's. They would need a `source` (and for sorts, the layout) on each step.
- **The real tray has no slot for Claude.** `ProposalRow` never shows `aiNote`, and has no place for a flag, a diff mark or "Ask Claude". `TidyRow` hard-codes "sorted by kind and year" and the "Built-in rule" tag. `TidyDrawer` hard-codes "Built-in rule: sorted by kind and year. No AI, offline." Atlas passes `agent={null}`, so `SourceTag` would say "Suggested by your AI agent", not Claude.
- **Privacy mode off contradicts today's rules.** AGENTS.md ("names, sizes, counts and dates, never file contents") and ADR 0003 ("File contents are never sent") need an explicit decision and an ADR. The hidden areas have to be enforced before Claude Code is asked: path deny rules in Claude Code's own read permissions plus Vacancy's list, not a prompt. macOS's Keychain and Windows' DPAPI folders are only the start of that list.
- **Guard refusals have no names.** The core's `AdvisorRejections` only counts (invalid reference, unsafe or changed, overlapping). Showing "Refused by the Guard: Windows Update downloads. It's inside Windows' own folder" needs per-item title, path and reason.
- **Claude's items need map tiles.** The prototype carves them out of their folder's tile (Android images out of `Sdk`), the way the core carves rule proposals. The core would need to do the same for AI proposals.
- **Claude's Tidy layouts change the reorganization decision** (AGENTS.md: by kind and by year). Its folder names need validating: length, no separators or `..`, no reserved Windows names. Reviewed names stay pinned as today.
- **Turning Claude off.** The prototype keeps Claude's items already in the tray (still marked), drops any unaccepted answer (pick, chats, explanations, layouts back to built-in), and keeps the privacy choice. This needs a decision.
- **Explain adds its suggestions to the tray at once, unticked.** The other option is an explicit "Add" per suggestion. Decide.
- **Hover cards can't hold buttons** (they follow the pointer), so asking about a folder from the map is a click on its tile. For ✳ folders at the top level that takes over the click-to-zoom, so you zoom with the region's header instead.
- **Not shown:** the scan plate's AI note (it still says "Built-in rules. AI stays off."; the prototype starts on the map), macOS (the same jobs without Tidy up), telling a slow reply apart from a hung one (the core's `slow_after`), and mapping `AdvisorError` kinds to copy. The prototype has one error line for all of them.

No user verdict yet. Don't promote this prototype or tag it until Zach chooses.
