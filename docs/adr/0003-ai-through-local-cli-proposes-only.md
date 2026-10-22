# 0003: AI runs through the user's local Claude Code or Codex CLI, and only proposes

Date: 2026-10-16. Status: accepted (Zach); narrowed to Claude Code only, with a privacy mode, by ADR 0008.

Built-in rules find known junk offline. When Claude Code (`claude`) or Codex (`codex`) is installed, Vacancy uses it for the judgment calls rules can't make: explaining big unknown folders and proposing a tidier layout for messy folders. Zach chose this over an API key, so the user's own subscription pays and no keys are stored.

- The AI **proposes; it never executes**. Claude Code runs headless with every tool disabled. Codex runs headless in its read-only sandbox from an empty working folder. Their structured output becomes plan items that pass the Guard (ADR 0002) and the user's review like any other.
- Only a minimized summary is sent: names, sizes, counts and dates. File contents are never sent. The user sees a privacy notice and can turn the AI off.
