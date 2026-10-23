# The check of Claude Code before privacy mode off lets Claude read (issue #29, Zach's decision)

A probe (`docs/proof/t12-invalid-settings-probe/`) showed that `claude -p` silently ignores a settings file it
rejects, deny rules and all. Vacancy checks its own settings before every call, but that can't prove a later Claude
Code takes them. So before the first privacy-off answer from a Claude Code executable, Vacancy makes one small real
call (`crates/vacancy-core/src/claude/canary.rs`, ADR 0008):

- a fresh throwaway folder in the app's data folder (`claude-check/check-...`), holding `control.txt` and a dummy
  Chromium profile, `BrowserData/Work` (`Preferences` and `History` markers, `Local State` above it), whose
  `Sessions/Session_1` is the canary. Both files hold fresh random words; the prompt names the files, never the words;
- the folder is scanned, so the canary is hidden by the browser-profile rules a real scan makes;
- the call is built by the same builder as every privacy-off call: the settings in a file, the same flags and
  environment, `blockReadsOutsideWorkingDirectories`, no `--model`, the folder as the read scope (as a drive, with
  the drive list's other drives and every mount point);
- it passes only if the control's words come back, the canary's don't, and Claude Code refuses the canary's read.

## The run: one real call

On 2026-10-23, with Claude Code `2.1.309 (Claude Code)` on Windows 11, and `ANTHROPIC_MODEL=haiku` set for that
test run only (the answer came from `claude-haiku-5-5`), through the opt-in test `live_claude_canary_check` in
`crates/vacancy-core/tests/canary.rs`, which makes the check exactly as the app does, with this computer's own drive
list and mounts:

```powershell
$env:VACANCY_LIVE_CANARY='1'; $env:ANTHROPIC_MODEL='haiku'
$env:VACANCY_PROBE_OUT='docs\proof\t12-canary-check\result.json'
cargo test -p vacancy-core --test canary live_claude_canary_check -- --nocapture
```

`result.json` keeps what came back: the Claude Code version, the verdict, the arguments after the system prompt, the
environment set and removed, the allow rule, `blockReadsOutsideWorkingDirectories`, how many deny rules (and how many
for the dummy profile), how many mount points were listed, Claude's structured answer, `permission_denials` and the
model. It keeps no session ids, costs or account details. The folder was Vacancy's own, under the test's temporary
folder, and was removed after the call.

| File | Result |
| --- | --- |
| `control.txt` | Read, quoted word for word (`open-<random>`) |
| `BrowserData\Work\Sessions\Session_1` | Refused: "File is in a directory that is denied by your permission settings." Listed in `permission_denials`. |

`passed` is true, `refusalSeen` is true, `canaryInOutput` is false (the canary's words appear nowhere in Claude
Code's output) and `controlInOutput` is true. The settings held 3,145 deny rules, 2,258 of them for the dummy profile
and the way down to the folder.

## What it proves, and what it doesn't

It proves that this Claude Code honours Vacancy's settings file, and with it the browser-profile deny rules and the
read scope. It doesn't prove that every rule on its own means what it says: the differential test
(`crates/vacancy-core/tests/rule_matcher.rs`) runs every kind of rule, the check's own settings among them, through
the matcher Claude Code bundles.

Not run live: macOS, and a failing Claude Code (the fake advisor covers each failing outcome in
`tests/canary.rs`).
