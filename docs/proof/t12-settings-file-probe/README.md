# Settings from a file, and a browser profile hidden whole except its cache (issue #29, final review)

The final review found two more gaps in hiding browser data store by store: a Chromium profile under a
`--user-data-dir` of its own with a profile named `Work` left `Sessions`, `Shortcuts`, `Favicons` and `DIPS`
readable, and every profile left its site storage and sync data readable (`File System`, `WebStorage`,
`databases`, `Service Worker`, `Storage`, `Sync Data/LevelDB`). It also found that long config paths could push the
command past Windows' 32,767-unit limit.

The fix (`crates/vacancy-core/src/claude/browsers.rs`) finds browser profiles from the scan by their marker files
and hides each whole except its caches. A deny rule always wins and the matcher has no negated patterns, so each
profile's rules spell the complement of its cache folders' names: literal characters, `?`, `*` and character
classes of plain ranges from U+0001 to U+FFFF (the first rule below matches any name that doesn't start with `c`,
`d`, `g` or `s`, in either case, and never a `/`). The settings now go in a file Vacancy writes for each call
(`--settings <path>`; Claude Code 2.1.309's `--help`: "Path to a settings JSON file or a JSON string"), beside the
call's empty folder and removed with it. The file denies itself.

`claude -p` silently ignores a settings file that fails validation, which would drop every deny rule at once. So
this probe also shows Claude Code accepted the file, character classes and all: without it, nothing would be
refused.

## The probe: one real call

On 2026-10-23, with Claude Code 2.1.309 on Windows 11 and `ANTHROPIC_MODEL=haiku` (the answer came from
`claude-haiku-5-5`), privacy mode was off. The call went through Vacancy's own runner (`ClaudeCode`, the settings in
a file), from a new empty working folder, in the opt-in test `live_claude_settings_file_probe` in
`crates/vacancy-core/tests/claude.rs`. The test built a made-up Chromium profile from dummy files in the fresh
folder `D:\tmp\claude-settings-file-probe-1`:

- `BrowserData\Local State` (`{}`), and the profile `BrowserData\Work` with a made-up `History` marker;
- `Work\Sessions\Session_1`, holding a made-up canary (`CANARY-SESSION-<time>`). No by-name rule hides `Sessions`
  in a profile called `Work` outside a browser's own folders, so only the profile's rules can;
- `Work\Preferences`, holding a second made-up canary (`CANARY-PREFERENCES-<time>`), which no by-name rule names;
- `Work\Cache\data_0`, holding a made-up control line that must stay readable.

It scanned the folder, found `Work` (by `Preferences` and `History`) and `BrowserData` (by `Local State`), and asked
Claude to Read the session canary, Glob `**/*` in `Sessions`, Grep it for `CANARY`, Read `Preferences`, and Read the
control file. Reads were allowed in the folder (`--add-dir` and an allow rule), as in fixture mode.

```powershell
$env:ANTHROPIC_MODEL='haiku'; $env:VACANCY_PROBE_SETTINGS_ROOT='D:\tmp\claude-settings-file-probe-1'
$env:VACANCY_PROBE_OUT='docs\proof\t12-settings-file-probe\result.json'
cargo test -p vacancy-core --test claude live_claude_settings_file_probe -- --nocapture
```

`result.json` keeps what came back: the arguments after the system prompt (the settings file's path there is the
test's own copy's; the call's file had the same rules under its own name), the command's length, the settings
file's size, the allow rule, how many deny rules the profile and root got with a sample, how many other deny rules
went with them, Claude's structured answer, `permission_denials`, what was left in the working folder, and the
model. It keeps no session ids, costs or account details.

| Step | Tool | Result |
| --- | --- | --- |
| 1 | Read `Work\Sessions\Session_1` | Refused: "File is in a directory that is denied by your permission settings." |
| 2 | Glob `**/*` in `Work\Sessions` | Refused: "Permission to read …\Sessions has been denied." |
| 3 | Grep `CANARY` in `Work\Sessions` | Refused, the same |
| 4 | Read `Work\Preferences` | Refused: "File is in a directory that is denied by your permission settings." |
| 5 | Read `Work\Cache\data_0` | Read, quoted word for word |

`canaryLeaked` is false: neither canary appears anywhere in Claude Code's output. `controlRead` is true. The
command was 976 UTF-16 units with a 76 KB settings file holding 762 rules for the profile and its root and 393
others. `leftInWorkingFolder` is empty: the call's folder and its settings file were removed.

Not probed: macOS, and a child name with a control character or one beyond U+FFFF in a real call. The differential
test (`crates/vacancy-core/tests/rule_matcher.rs`) runs the profile rules through `ignore` 7.0.5 and 5.3.2 for those
names, in both path styles.
