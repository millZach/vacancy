# Glob characters in a moved Claude Code folder (issue #29, second review)

The second review found that Vacancy wrote real paths into Claude Code's permission rules unescaped. Rules you
write yourself are gitignore patterns, and Claude Code doesn't escape them (its permissions docs, "Special
characters"). So `CLAUDE_CONFIG_DIR=D:\AI\[work]\claude-config` became `Read(//d/AI/[work]/claude-config/**)`, where
`[work]` is a character class matching `w`, `o`, `r` or `k`, and the deny missed the folder. `CODEX_HOME`, the
fixture folder's allow rule and every other rule built from a real path had the same bug.

The fix (`rule_literal` and `rule_path` in `crates/vacancy-core/src/claude/runner.rs`) escapes every name taken from
a real path before the rule's own wildcards go on. A backslash goes before `\`, `[`, `]`, `*`, `{`, `}`, `!` and
`#`, and before a trailing space. This covers deny and allow rules alike: config folders, as set and as resolved,
relative ones, the fixture folder and drive roots. The always-hidden places' own patterns keep their intended
wildcards. Parentheses need no escaping (the docs). `?` stays as it is, because `\?` matches nothing in this matcher,
not even a `?` (see below). As a wildcard, a `?` makes a deny rule hide a little more, never less, and a root whose
path holds one gets no allow rule. Windows names can't hold `?`, `*` or `\`. `--add-dir` takes a path, not a
pattern, so it is never escaped; the probe reads through a bracketed `--add-dir` root to show it.

## The matcher, offline

`ignore-check.cjs` runs patterns through the `ignore` npm package (5.3.2 here), the gitignore matcher Claude Code
2.1.309 bundles (its binary carries the package's own error text). `ignore-check.txt` is its output. The old rule
misses `[work]` and matches `w` instead; the escaped one matches only `[work]`. `\?` matches nothing, because the
package's rule for `?` is `(?!\\)\?` (a lookahead where a lookbehind was meant), so Vacancy never writes it.

## The probe: one real call

On 2026-10-22, with Claude Code 2.1.309 on Windows 11 and `ANTHROPIC_MODEL=haiku` (the answer came from
`claude-haiku-5-5`), privacy mode was off. The call went through Vacancy's own runner (`ClaudeCode` with
`invocation_in`), from a new empty working folder, in the opt-in test `live_claude_glob_probe` in
`crates/vacancy-core/tests/claude.rs`:

- `D:\tmp\claude-glob-probe-1\[work]\cfg\canary.txt` held a made-up canary
  (`CANARY-GLOB-7Q4K-20261022 (made-up probe text, not a secret)`).
- `[work]\cfg` was passed to the rule builder as the configured Claude Code folder.
- Claude could read `[work]`, through `--add-dir` and an allow rule.
- `[work]\open.txt` held a made-up control line that should stay readable.

Claude was asked to Read the canary, Glob `**/*` in `cfg`, Grep it for `CANARY`, and Read the control file.

```powershell
$env:ANTHROPIC_MODEL='haiku'; $env:VACANCY_PROBE_GLOB_ROOT='D:\tmp\claude-glob-probe-1'
$env:VACANCY_PROBE_OUT='D:\tmp\claude-glob-probe-1-out.json'
cargo test -p vacancy-core --test claude live_claude_glob_probe -- --nocapture
```

`probe-1.json` keeps what came back: `addDir`, the allow rule, the two deny rules for the configured folder (with
how many other deny rules went with them), the environment, Claude's structured answer, `permission_denials` and
the model. It keeps no session ids, costs or account details.

| Step | Tool | Rule it met | Result |
| --- | --- | --- | --- |
| 1 | Read `cfg\canary.txt` | `Read(//d/tmp/claude-glob-probe-1/\[work\]/cfg/**)` | Refused: "File is in a directory that is denied by your permission settings." |
| 2 | Glob `**/*` in `cfg` | the same | Refused: "Permission to read …\cfg has been denied." |
| 3 | Grep `CANARY` in `cfg` | the same | Refused, the same |
| 4 | Read `[work]\open.txt` | `Read(//d/tmp/claude-glob-probe-1/\[work\]/**)` and `--add-dir D:\tmp\claude-glob-probe-1\[work]` | Read, quoted word for word |

`canaryLeaked` is false: the canary appears nowhere in Claude Code's output. `controlRead` is true. The escaped deny
rule held for Read, Glob and Grep. The control read shows that a bracketed folder passed with `--add-dir` as it is
spelled works, and that the escaped allow rule doesn't get in its way.

Not probed: macOS, and `*`, `{}` and `!` in a real call. The unit tests pin their escaping
(`real_names_are_literals_in_a_rule`, `moved_config_folders_with_glob_characters_are_denied_as_spelled`), and the
offline check shows how the matcher reads them.
