// Runs Vacancy's Claude Code permission rules through the matcher Claude Code itself uses, for the differential
// test `crates/vacancy-core/tests/rule_matcher.rs` (ADR 0008). It reads a JSON file of checks, each a set of
// `Read(...)` rules of one kind (deny or allow), a real path, the platform's path style, and whether the set must
// match the path, and prints what failed as JSON; it exits 1 if anything did.
//
// The matcher is the `ignore` package: 7.0.5, which Claude Code 2.1.309 bundles (identified from the installed
// binary's embedded code: `checkIgnore`, the `setupWindows` symbol, and 7.0.5's replacers), and 5.3.2, which the
// review of #29 checked against. Both are pinned in package.json under aliases. Each is loaded once per platform,
// with `process.platform` set to Windows' or macOS's, because the package changes how it reads paths on Windows.
//
// Before a rule reaches the matcher, Claude Code (2.1.309, from its embedded code) does this, and so does this
// script: it takes the rule's content between `Read(` and `)`; a content from `//` is from the file system's root,
// and on Windows `//c/...` is from the drive `C:\`; runs of `/` become one; a trailing `/**` comes off (the folder's
// pattern then matches everything in it); a pattern the matcher would skip (blank, a comment, a trailing
// backslash) or can't compile is dropped if it allows and rewritten if it denies, which Vacancy must never cause;
// and the matcher is made with its defaults (case-insensitive) and given the pattern in an array. A path is then
// tested relative to the rule's root, with `/` between names. This script also gives each pattern as text, the
// way some of Claude Code's paths do, and requires the same answer.
//
// Usage: node tools/rule-matcher.mjs <checks.json>

import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';

const require = createRequire(import.meta.url);

const PACKAGES = { '7.0.5': 'ignore-claude-code', '5.3.2': 'ignore-5' };

/** The `ignore` factory of `pkg`, as it is on `platform` ('win32' or 'darwin'). */
function load(pkg, platform) {
  const file = require.resolve(`${pkg}/index.js`);
  const version = JSON.parse(readFileSync(require.resolve(`${pkg}/package.json`), 'utf8')).version;
  const module = { exports: {} };
  const sandbox = { module, exports: module.exports, require, process: { platform, env: {} }, Symbol, Object };
  vm.runInNewContext(readFileSync(file, 'utf8'), sandbox, { filename: file });
  return { factory: module.exports, version };
}

/** What Claude Code makes of the rule `rule` on `windows` or not: its root and pattern, or why it can't. */
function prepare(rule, allow, windows) {
  if (!rule.startsWith('Read(') || !rule.endsWith(')')) return { error: 'not a Read(...) rule' };
  const content = rule.slice(5, -1);
  if (!content.startsWith('//')) return { error: 'not a rule from the root (//)' };
  let rel = content.slice(1);
  let root = '/';
  const drive = windows && /^\/[a-z]\//i.exec(rel);
  if (drive) {
    root = `${rel[1].toUpperCase()}:\\`;
    rel = rel.slice(2);
  }
  rel = rel.replace(/\/{2,}/g, '/');
  if (rel.includes('\uFEFF')) return { error: 'a byte-order mark, which Claude Code rewrites' };
  let pattern = rel;
  if (pattern.endsWith('/**')) {
    const r = pattern.slice(0, -3);
    pattern = /[^/]/.test(r) ? (r.includes('/') || !allow || /^[!#]/.test(r) ? r : `/${r}`) : '/**';
  }
  if (/^\s*$|^#|(?:^|[^\\])\\$/.test(pattern)) {
    return { error: 'a pattern the matcher skips (blank, a comment, or a trailing backslash)' };
  }
  return { root, pattern };
}

/** `path` relative to `root` on Windows or not, with `/` between names, or null when it isn't under it. */
function relative(path, root, windows) {
  if (!windows) {
    if (!path.startsWith('/')) throw new Error(`not a Unix path from the root: ${JSON.stringify(path)}`);
    return path.slice(1).split('/').filter(Boolean).join('/');
  }
  const m = /^([a-z]):[\\/](.*)$/is.exec(path);
  if (!m) throw new Error(`not a Windows path from a drive: ${JSON.stringify(path)}`);
  const names = m[2].split(/[\\/]/).filter(Boolean);
  if (root === '/') return [m[1].toLowerCase(), ...names].join('/');
  if (root[0].toLowerCase() !== m[1].toLowerCase()) return null;
  return names.join('/');
}

const checks = JSON.parse(readFileSync(process.argv[2], 'utf8')).checks;
const failures = [];
const versions = [];
for (const [want, pkg] of Object.entries(PACKAGES)) {
  const matchers = { windows: load(pkg, 'win32'), unix: load(pkg, 'darwin') };
  if (matchers.unix.version !== want) {
    failures.push({ error: `${pkg} is ignore ${matchers.unix.version}, not ${want}: run npm ci` });
    continue;
  }
  versions.push(want);
  // One matcher per pattern, as an array and as text, made once: a browser profile's rules are hundreds of
  // patterns, each checked against many paths. A matcher's answer for a path doesn't depend on what it answered
  // before.
  const made = { windows: new Map(), unix: new Map() };
  const matcherOf = (style, pattern) => {
    let m = made[style].get(pattern);
    if (!m) {
      const ignore = matchers[style].factory;
      m = { inArray: ignore().add([pattern]), asText: ignore().add(pattern) };
      made[style].set(pattern, m);
    }
    return m;
  };
  for (const check of checks) {
    const windows = check.style === 'windows';
    const allow = check.kind === 'allow';
    let matched = false;
    for (const rule of check.rules) {
      const prepared = prepare(rule, allow, windows);
      if (prepared.error) {
        failures.push({ version: want, rule, error: prepared.error });
        continue;
      }
      const path = relative(check.path, prepared.root, windows);
      if (path === null || path === '') continue;
      let inArray;
      let asText;
      try {
        const m = matcherOf(check.style, prepared.pattern);
        inArray = m.inArray.ignores(path);
        asText = m.asText.ignores(path);
      } catch (e) {
        failures.push({ version: want, rule, path: check.path, error: String(e) });
        continue;
      }
      if (inArray !== asText) {
        failures.push({ version: want, rule, path: check.path, error: 'the pattern means another thing as text' });
      }
      if (inArray) matched = true;
    }
    if (matched !== check.expect) {
      failures.push({
        version: want,
        kind: check.kind,
        style: check.style,
        path: check.path,
        why: check.why,
        expected: check.expect,
        rules: check.rules.length > 4 ? `${check.rules.length} rules` : check.rules,
      });
    }
  }
}
process.stdout.write(JSON.stringify({ versions, checks: checks.length, failures }, null, 1));
process.exit(failures.length ? 1 : 0);
