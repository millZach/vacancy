# 0008: Live Claude advisor through the user's Claude Code, with a privacy mode

Date: 2026-10-22. Status: accepted (Zach). Supersedes the Oct 17 deferral of CLI AI (`docs/advisor-status.md`) and narrows ADR 0003 to Claude Code only.

Zach turned live AI on. Vacancy calls the user's own Claude Code (`claude`), through their subscription. There is no Codex and no API key. AI is **off until the user turns it on**, after a privacy notice. The user can turn it off again any time.

## What Claude does

All of these appear in a docked Claude side panel (prototype `prototype/ai-advisor`, variant B):

- **Review my plan:** a second opinion that flags proposals it thinks are risky, with a reason. It adds nothing.
- **Let Claude pick:** one button and no goal. Claude ticks and unticks the rules' proposals and may add its own. The user sees the changes against their plan, then picks Keep mine or Use Claude's plan.
- **Ask Claude:** a follow-up chat about any proposal or folder: what it is, what you'd lose, whether it comes back.
- **Explain big folders:** big folders the rules don't recognize get an explanation. Any removable items Claude suggests are added to the plan unticked.
- **Suggest Tidy up layouts:** Claude's sorting of the loose files of Downloads, Desktop and Documents, with Sort later, as an alternative to the built-in layout. Moves stay journaled and undoable, and the 512-file cap, pinned names and delete-wins-over-sort still apply.

## What Claude can never do

- **Claude proposes; it never executes.** Its output is structured data. Every item it adds is marked "Suggested by Claude", starts unticked with risk Review, must point at something the scan saw, and must pass the Guard (ADR 0002). Invalid items are dropped and the reason is shown. The executor treats Claude's items like any other. Turning Claude off keeps its items already in the plan.
- **Only the scan it was about.** Every command that touches Claude's part of a scan names the scan its map came from: a job, a chat, Keep mine taking items out, and a layout picked. The core refuses it once the drive has a newer scan (or the plan ran, or the drive went away). Item ids like `claude-d17` come again in the next scan, so a command for a replaced scan must never touch the new one. The UI never starts one for a scan that is gone. Every complete map the core gives out for a scan carries one revision, counted per scan across the scan's own map (0), the duplicate search's and each Claude answer's. The UI shows a map only if its revision is above the one it shows, whichever kind each is, so a slow reply never puts back an older map. The duplicate search's end is kept apart from its map: a map refused as stale still ends the search. One Let Claude pick runs at a time: while one runs, the chip and every pick card's Try again wait, so the answers come in the order they were asked. The newest answered pick is the one whose answer came last (one set aside included). The panel keeps it the moment each answer is handled, never from what was last drawn, and that one pick decides both which card offers Keep mine and whose Keep mine may put the plan back. Keep mine puts the plan back only if, once its items are out, its card is still the newest answered pick and the plan is still as its answer left it; the drive's plan checks the pick's version and ticks as it puts the plan back.
- **Claude Code runs locked down.** The command is `claude -p --tools <set> --setting-sources "" --strict-mcp-config --disable-slash-commands --no-session-persistence --permission-prompts none --system-prompt <Vacancy's> --output-format json --json-schema <schema>`, with these settings:
  - It runs from an empty folder Vacancy owns.
  - `ANTHROPIC_API_KEY` is removed from its environment.
  - `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` are set.
  - `CLAUDE_CODE_DISABLE_ATTACHMENTS=1` and `CLAUDE_CODE_DISABLE_FILE_MENTIONS=1` are set in both modes, so an `@` path typed in a chat is never expanded into a file's contents (review of #29; `docs/proof/t12-attach-probes/`).
  - The always-hidden deny rules are passed with `--settings` in both modes: with privacy mode on, as defence in depth.

  So the user's hooks, plugins, MCP servers, CLAUDE.md files and settings don't load, and no transcript is kept. `--bare` can't be used because it never reads the subscription login.

## Privacy mode

- **On**, the default when Claude is turned on: `--tools ""`. Claude gets names, sizes, counts and dates from the scan only, never file contents.
- **Off** (the user's choice, after a confirm, and remembered): `--tools Read,Glob,Grep`. Claude can read files anywhere on the PC. It still can't write, delete, run commands or go online. In fixture mode its reads are confined to the fixture folder (on a case-sensitive Mac volume, also to a sibling spelled like it in other case; see the known limit below).
- **Always hidden, even with privacy mode off.** Vacancy passes Claude Code `permissions.deny` rules for Read, Glob and Grep, so Claude Code enforces them. Vacancy also never puts these places' contents in a prompt:
  - **keys and sign-ins:** SSH and GPG keys, cloud and developer tokens (.aws, .azure, gh, npm, Docker), `.env` files, Claude Code's and Codex's own folders (`~/.claude`, `~/.claude.json`, `~/.codex`, and wherever `CLAUDE_CONFIG_DIR` and `CODEX_HOME` move them; Vacancy never reads inside them), Windows Credentials and DPAPI Protect folders, the macOS Keychain;
  - **browser data:** saved passwords and the keys that unlock them, cookies and site sign-ins (local, session and IndexedDB storage, Firefox's site storage), history, sessions and bookmarks, autofill and form history, and extensions' own storage (where wallet extensions keep their vaults), in Edge, Chrome, Brave, Vivaldi, Opera and other Chromium browsers, Firefox and Safari, each database with its `-journal`, `-wal` and backup siblings (their caches stay readable). A Chromium profile can have any name (`--profile-directory=Work`) and a Firefox profile can live in any folder, so these are hidden by the stores' own names (`Login Data`, `Bookmarks`, `IndexedDB`, `places.sqlite`, `key4.db`, `storage/default` and the rest) wherever they are, never by a profile's name or place. The few names that are also ordinary names (`Sessions`, `Shortcuts`, `Favicons`, `DIPS`) are hidden at any depth below where Chromium browsers keep their profiles (`User Data`, `Opera Software`, a Mac's `Application Support`) and in profiles Chromium names (`Default`, `Profile *`, `* Profile`) anywhere;
  - **password managers and wallets:** 1Password, Bitwarden, KeePass `.kdbx` files and crypto wallets.

  Messages and mail are not hidden; Zach chose that.

  Every rule built from a real path is a literal or fails safe, deny and allow rules alike: moved Claude Code and Codex folders (as set and as resolved), the fixture folder and drive roots. Claude Code doesn't escape rules you write yourself, and its matcher (the `ignore` package, 7.0.5 in Claude Code 2.1.309) can't spell every name:
  - **Spelled exactly:** a name whose characters are all printable ASCII other than `\`, `?` and `*`. A backslash goes before `[`, `]`, `{`, `}`, `!` and `#`, and before a trailing space; the matcher escapes the rest itself.
  - **Never spelled:** line breaks, CR, tab and every other whitespace, a Unix backslash, `?`, `*`, letters beyond ASCII and control characters. No allow rule is made for such a path. A deny rule writes `*` for each run of them, so it hides the folder and maybe a sibling, never less.
  - **No rule at all:** a path that isn't Unicode, or a moved folder on Windows that isn't from a drive letter. Then privacy mode off can't let Claude open files: every call runs as if privacy mode were on, and the panel and each answer say why. In fixture mode, a fixture folder no allow rule spells exactly refuses privacy mode off.
  - **Named places:** each rule is given twice (`//**/x` and `//*/**/x`), because the matcher's leading `**` stops at a line break in a folder's name.
  - **Proof:** a differential test (`crates/vacancy-core/tests/rule_matcher.rs`, through `tools/rule-matcher.mjs`) runs every kind of rule, for over a hundred adversarial names in Windows' and Unix's path styles, through `ignore` 7.0.5 and 5.3.2. It prepares each rule as Claude Code 2.1.309's code does (read from its installed program, not a live call). It checks that each deny rule matches its target, and that each allow rule matches inside its root and nothing beside it, except a sibling spelled like the root in other case (the known limit below).
  - **The tested version:** the rules were verified against Claude Code 2.1.309, which bundles `ignore` 7.0.5. When Claude Code updates its matcher, the model in `tools/rule-matcher.mjs` (which `ignore` it bundles, and how it prepares a rule) needs rechecking. Zach chose to note this only: Vacancy makes no runtime version check.
  - **Known limit, case-sensitive Mac volumes:** the matcher compares case-insensitively, so an allow rule also covers a sibling folder whose name differs from its root only in case. On a case-sensitive Mac volume in fixture mode with privacy mode off, the fixture folder's allow rule therefore also lets Claude read such a sibling. Zach accepted this: it happens in fixture (test) mode only, and normal mode's allow rules are whole drives. (A deny rule matching other case only hides more.)

  `--add-dir` takes paths, not patterns, and is never escaped. A real probe showed a `[work]` folder denied as spelled, for Read, Glob and Grep (`docs/proof/t12-glob-probe/`).
- **Chats keep their mode, and a change of mode ends them.** The core keeps every chat, per scan, by an id it gives out; the UI sends only that id and the person's new question. A chat's first answer can come before its id does; its follow-ups wait for the id, so a follow-up never opens a second chat. A chat holds the privacy mode it began in, and each of its turns the mode it was made in. Changing privacy mode, or turning Claude off, ends every chat in the core in the same step as the new setting is kept (a request is prepared wholly before or wholly after). The panel ends its own and asks nothing until the core has answered. A question to an ended chat, or one in the other mode, is refused and sends nothing; the panel asks it again in a new chat. So a call with privacy mode on never holds a chat turn from privacy mode off. It sends Claude's items made with privacy mode off by their name in the scan only, never with Claude's note about them.
- **Still sent either way:** Vacancy's prompt and Claude's replies, and the account email Claude Code itself adds. Zach has zero data retention on.
- **A risk accepted with privacy mode off:** a file Claude reads can carry text meant to steer it. That is safe because Claude can't act, the Guard still applies, and its items start unticked behind the user's one confirmation.

## States

The advisor shows one of these states:

- **Not found:** no `claude` was found. Vacancy looks in `%USERPROFILE%\.local\bin\claude.exe` and the PATH on Windows, and `~/.local/bin/claude`, Homebrew and npm global plus the login shell's PATH on macOS. It spawns the `.exe`, never a `.cmd`.
- **Not signed in:** `claude auth status` fails.
- **The sign-in check timed out, or failed:** `claude auth status` didn't answer in 30 seconds, or couldn't run. Each shows its own state with why; neither is shown as "not signed in".
- **Off, on, working:** working can be cancelled, and slow shows after a while.
- **Timed out, and error:** each with Claude's message.
