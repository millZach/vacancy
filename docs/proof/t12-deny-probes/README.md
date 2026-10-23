# Claude Code deny-rule probes (issue #29)

Real `claude -p --model haiku` calls (Claude Code 2.1.308, Windows 11), 2026-10-22, each from a new empty folder `D:\tmp\claude-deny-probe-<n>`, against dummy secrets made for the probe in `D:\tmp\claude-deny-probe-data` (no real secret was read). The command was Vacancy's privacy-off command with `--model haiku` added: `probe.sh` (reads confined to the data folder, like fixture mode) and `probe-whole.sh` (the whole drive, like real mode). The prompt went over stdin. `probe-<n>.json` keeps each result's `permission_denials`, `result` and model; session ids and costs are left out.

The settings passed with `--settings`:

```json
{"permissions":{"allow":["Read(//d/tmp/claude-deny-probe-data/**)"],
 "deny":["Read(//**/.ssh/**)","Read(//d/tmp/claude-deny-probe-data/home/.aws/**)","Read(//**/Login Data)"]}}
```

| Probe | Asked | Found |
| --- | --- | --- |
| 1 | Read a readable note and the three secrets by absolute Windows path | The note read; all three secrets refused ("File is in a directory that is denied by your permission settings"). All three rule forms work on Windows: `//**/name/**`, an absolute `//d/...` path, and a file name with a space. |
| 2 | Glob `**/*` and Grep `PROBE-` over the data folder; Grep in `.ssh`; Glob in `.aws` | Glob listed only the note; Grep found only the note's line; Grep and Glob aimed at the denied folders were refused. A `Read(...)` deny rule covers Glob and Grep. |
| 3 | Read the secrets as `D:\TMP\...\.SSH\ID_RSA`, `SSH~1\id_rsa` (8.3 name), forward slashes with `.AWS`, lower-case `login data`, and `notes\..\.ssh\id_rsa` | All refused: matching is case-insensitive on Windows, `..` is resolved first, and a short name is refused outright as a suspicious Windows path. |
| 4 | Read, Glob and Grep outside the allowed folder; Glob `.ssh/*` and Grep with glob `**/.ssh/*` from the data folder; list the tools | Outside reads refused ("Path is outside allowed working directories", nobody can approve with `--permission-prompts none`); the `.ssh` patterns found nothing; tools were Glob, Grep and Read only. This is fixture mode's confinement. |
| 5 | `probe-whole.sh` (`--add-dir D:\`, `Read(//d/**)`): read the outside note, the secrets, Glob and Grep the data folder | The outside note read (whole-drive reading works); `.ssh` and `Login Data` still refused; Glob and Grep hid them. |

One finding changed the runner: `--add-dir` takes any number of values, so a prompt given after it as an argument is swallowed ("Input must be provided either through stdin or as a prompt argument"). Vacancy sends every prompt over stdin.

Not probed: macOS. There the same rules apply from `//**/`, but whether matching is case-insensitive on a case-insensitive APFS volume was not checked.
