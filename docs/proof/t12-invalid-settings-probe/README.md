# A settings file Claude Code drops, and one too big for it (issue #29, final review)

The final review found that the settings Vacancy hands Claude Code (`--settings <file>`) grow with every browser
profile a scan finds (84 profiles came to 2.1 MB), and asked what `claude -p` does with a settings file it can't
take: stop, or carry on without it, deny rules and all.

## What the docs and help say (Claude Code 2.1.309)

- **Too big: it stops.** `claude --help`, `--settings`, and the CLI reference: "The file must be a regular file no
  larger than 2 MiB." The errors page, "Settings file exceeds the 2MiB limit": "The file you passed to `--settings`
  is larger than 2 MiB, so `claude` exits with code 1 at startup instead of loading it."
- **Invalid: it carries on.** `claude --help` (under the workspace trust note for `-p`): "Settings files that fail
  validation are silently ignored in this mode (no error dialog is shown)." The settings page, "Fix a broken settings
  file": "A `-p` run shows no dialog. [...] Claude Code skips the broken file or values and continues with the rest",
  and a "Settings Warning" is for "only individual entries [that] fail, such as a malformed permission rule. Claude
  Code skips those values and keeps the rest of the file in effect."

## The probe: one real call

On 2026-10-23, with Claude Code 2.1.309 on Windows 11 and `ANTHROPIC_MODEL=haiku` (the answer came from
`claude-haiku-5-5`), `probe.sh` made the fresh folder `D:\tmp\claude-invalid-settings-probe-1` with an empty
working folder `work\`, a made-up canary `data\canary.txt` (`CANARY-INVALID-SETTINGS-<time>`), a control line
`data\control.txt`, and `settings.json` beside them:

```json
{"permissions":{"allow":["Read(//d/tmp/claude-invalid-settings-probe-1/data/**)"],
 "deny":["Read(//d/tmp/claude-invalid-settings-probe-1/data/canary.txt)","Read(//d/tmp/claude-invalid-settings-probe-1/settings.json)"],
 "defaultMode":12345}}
```

`permissions.defaultMode` must be one of the mode names, so the schema rejects the file. The call was Vacancy's
privacy-off command (the locked-down flags and environment, `--tools Read,Glob,Grep`, `--settings` naming the file,
`--add-dir data`), from `work\`, the prompt over stdin, asking Claude to Read the canary and then the control file.

| | Result |
| --- | --- |
| Exit code | 0 |
| stderr | empty |
| `is_error`, `subtype` | `false`, `success` |
| `permission_denials` | none |
| The canary | read and quoted word for word (`canaryLeaked: true`) |
| The control file | read |

So Claude Code dropped the whole file, deny rule and all, and said nothing: not on stderr, not in the exit code, not
in its JSON result (`outputKeys` in `result.json` lists every field; none is about settings). `result.json` keeps
what came back, without session ids, costs or account details. The canary was a made-up string in a folder made for
the probe; no real file was read.

## What Vacancy does about it

There is no cheap, reliable way to ask Claude Code afterwards whether it took the file (`claude doctor` is a
separate, interactive check of the user's own configuration), so the guard is Vacancy's own check before every call
(`runner::check_settings`), which refuses to start Claude Code when the settings:

- are over 1.5 MiB (`MAX_SETTINGS_BYTES`), well below the 2 MiB at which Claude Code stops;
- hold anything but `permissions`, with `allow` and `deny` lists of `Read(/...)` rules, each on one line, at least
  one deny rule, and `blockReadsOutsideWorkingDirectories: true`. Vacancy writes nothing else, so anything else is a
  mistake of its own.

With privacy mode off, a scan whose rules would be over the cap runs every call as if privacy mode were on, and the
answer and the panel say why (`claude::access_on`); no rule is left out to make them fit. A Chromium root's
profiles share one set of rules (`browsers::profile_names_rules_in`), so 84 profiles in one root no longer come near
the cap; hundreds of profiles that each need their own (Electron apps' data folders) can.

What the check can't see: a single rule Claude Code thinks malformed is skipped on its own (a "Settings Warning").
Every kind of rule Vacancy builds is run through Claude Code's matcher in the differential test
(`crates/vacancy-core/tests/rule_matcher.rs`), and the real call in `docs/proof/t12-settings-file-probe/` showed
the profile rules, character classes and all, taken and enforced.
