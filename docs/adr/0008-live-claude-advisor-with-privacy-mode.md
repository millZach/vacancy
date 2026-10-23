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
- **Claude Code runs locked down.** The command is `claude -p --tools <set> --setting-sources "" --strict-mcp-config --disable-slash-commands --no-session-persistence --permission-prompts none --system-prompt <Vacancy's> --output-format json --json-schema <schema>`, with these settings:
  - It runs from an empty folder Vacancy owns.
  - `ANTHROPIC_API_KEY` is removed from its environment.
  - `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` are set.
  - `CLAUDE_CODE_DISABLE_ATTACHMENTS=1` and `CLAUDE_CODE_DISABLE_FILE_MENTIONS=1` are set in both modes, so an `@` path typed in a chat is never expanded into a file's contents (review of #29; `docs/proof/t12-attach-probes/`).
  - The always-hidden deny rules are passed with `--settings` in both modes: with privacy mode on, as defence in depth.

  So the user's hooks, plugins, MCP servers, CLAUDE.md files and settings don't load, and no transcript is kept. `--bare` can't be used because it never reads the subscription login.

## Privacy mode

- **On**, the default when Claude is turned on: `--tools ""`. Claude gets names, sizes, counts and dates from the scan only, never file contents.
- **Off** (the user's choice, after a confirm, and remembered): `--tools Read,Glob,Grep`. Claude can read files anywhere on the PC. It still can't write, delete, run commands or go online. In fixture mode its reads are confined to the fixture folder.
- **Always hidden, even with privacy mode off.** Vacancy passes Claude Code `permissions.deny` rules for Read, Glob and Grep, so Claude Code enforces them. Vacancy also never puts these places' contents in a prompt:
  - **keys and sign-ins:** SSH and GPG keys, cloud and developer tokens (.aws, .azure, gh, npm, Docker), `.env` files, Claude Code's and Codex's own folders (`~/.claude`, `~/.claude.json`, `~/.codex`, and wherever `CLAUDE_CONFIG_DIR` and `CODEX_HOME` move them; Vacancy never reads inside them), Windows Credentials and DPAPI Protect folders, the macOS Keychain;
  - **browser data:** saved passwords and the keys that unlock them, cookies and site sign-ins (local and session storage), history, sessions and bookmarks, and autofill and form history, in Edge, Chrome, Brave, Vivaldi, Firefox and Safari, each database with its `-journal`, `-wal` and backup siblings (their caches stay readable);
  - **password managers and wallets:** 1Password, Bitwarden, KeePass `.kdbx` files and crypto wallets.

  Messages and mail are not hidden; Zach chose that.
- **Still sent either way:** Vacancy's prompt and Claude's replies, and the account email Claude Code itself adds. Zach has zero data retention on.
- **A risk accepted with privacy mode off:** a file Claude reads can carry text meant to steer it. That is safe because Claude can't act, the Guard still applies, and its items start unticked behind the user's one confirmation.

## States

The advisor shows one of these states:

- **Not found:** no `claude` was found. Vacancy looks in `%USERPROFILE%\.local\bin\claude.exe` and the PATH on Windows, and `~/.local/bin/claude`, Homebrew and npm global plus the login shell's PATH on macOS. It spawns the `.exe`, never a `.cmd`.
- **Not signed in:** `claude auth status` fails.
- **The sign-in check timed out, or failed:** `claude auth status` didn't answer in 30 seconds, or couldn't run. Each shows its own state with why; neither is shown as "not signed in".
- **Off, on, working:** working can be cancelled, and slow shows after a while.
- **Timed out, and error:** each with Claude's message.
