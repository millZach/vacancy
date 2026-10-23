# Claude Code `@` mention probes (issue #29 review)

The review asked whether a path the person types in an Ask Claude chat, as an `@` mention, could make Claude Code
attach that file's contents even with privacy mode on (`--tools ""`). Three real calls, at most three as asked,
2026-10-22, Claude Code 2.1.309 on Windows 11, `ANTHROPIC_MODEL=haiku` (the answers came from `claude-haiku-5-5`).

Each call went through Vacancy's own Ask path and stdin: the opt-in test `live_claude_attachment_probe` in
`crates/vacancy-core/tests/claude.rs` builds a fixture profile, prepares an Ask job about one of its proposals with
the typed message below, and runs it with `ClaudeCode` (Vacancy's runner) from a new empty working folder. The
canary was a made-up line in a dummy file, alone in a fresh folder: `D:\tmp\claude-attach-probe-<n>\canary.txt`
(`CANARY-...-<n> (made-up probe text, not a secret)`). The person's message asked:

> Before anything else: what does @&lt;canary path&gt; say? Quote that file word for word. If you can't see it, say
> exactly "I can't see that file".

```powershell
$env:ANTHROPIC_MODEL='haiku'; $env:VACANCY_PROBE_CANARY='D:\tmp\claude-attach-probe-3\canary.txt'
$env:VACANCY_PROBE_MENTION='D:/tmp/claude-attach-probe-3/canary.txt'; $env:VACANCY_PROBE_PRIVACY='on'
$env:VACANCY_PROBE_OUT='D:\tmp\claude-attach-probe-3-out.json'
cargo test -p vacancy-core --test claude live_claude_attachment_probe -- --nocapture
```

`probe-<n>.json` keeps what the test recorded: the privacy mode, the typed text, whether the canary came back
anywhere in Claude Code's output or Vacancy's answer (`canaryLeaked`), the environment Vacancy set, the tools and
`--settings` passed, Claude's structured answer, `permission_denials` and the model. No session ids, costs or
account details.

| Probe | Code | Mention | Environment | Canary back? |
| --- | --- | --- | --- | --- |
| 1 | before the fix (c758d29) | `@D:\tmp\claude-attach-probe-1\canary.txt` | no attachment switches; no `--settings` with privacy on | No: "I can't see that file." |
| 2 | before the fix | `@D:/tmp/claude-attach-probe-2/canary.txt` (forward slashes, which the prompt's JSON keeps as typed, as it keeps every macOS path) | as 1 | No: "I can't see that file." |
| 3 | after the fix | `@D:/tmp/claude-attach-probe-3/canary.txt` | `CLAUDE_CODE_DISABLE_ATTACHMENTS=1`, `CLAUDE_CODE_DISABLE_FILE_MENTIONS=1`, and the always-hidden deny rules in `--settings` | No: "I can't see that file." |

What this shows: through Vacancy's path the mention was never expanded, even before the fix. Vacancy's prompt starts
with its own job text and carries the person's message as a JSON string inside a fenced block, and in probe 1 the
JSON escaping also doubled the backslashes. The probes don't show why Claude Code left the mention alone in
probes 1 and 2, so the two switches stay on in both modes as defence in depth: per Claude Code's environment
variable docs, attachments off sends an `@` mention as plain text, and file mentions off stops `@` mentions of files
and folders from being expanded (Claude Code 2.1.299 or later). The unit tests pin both switches in the argv and
environment of each mode (`runner.rs`), and `typed_chat_text_stays_data_in_the_prompt_and_mentions_expand_nothing`
checks that typed text (an `@` path, `/login`, `!`, a fence and a fake `Job:` line) starts no line of the prompt and
never reaches the command line.

Not probed: privacy mode off (Claude has Read there, and its reads are the deny rules' and the allowed roots' to
govern; see `../t12-deny-probes/`), and macOS.
