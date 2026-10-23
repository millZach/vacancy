// Which paths a Read rule matches in the gitignore matcher Claude Code bundles (the `ignore` npm package; Claude
// Code 2.1.309's binary carries its messages). Offline: no Claude call. Usage, from the repo root:
//   node docs/proof/t12-glob-probe/ignore-check.cjs node_modules/ignore
// Each row: a rule's pattern (as Claude Code sees it after `Read(//`), a path from the root, and whether it matches.
const ignore = require(require('path').resolve(process.argv[2]));
const B = String.fromCharCode(92);
const e = (s) => s.split('~').join(B); // `~` stands for a backslash
const canary = 'd/tmp/claude-glob-probe-1/[work]/cfg/canary.txt';
const rows = [
  ['before the fix: [work] as written', 'd/tmp/claude-glob-probe-1/[work]/cfg/**', canary],
  ['before the fix: what it matched instead', 'd/tmp/claude-glob-probe-1/[work]/cfg/**', 'd/tmp/claude-glob-probe-1/w/cfg/canary.txt'],
  ['after the fix: \\[work\\]', e('d/tmp/claude-glob-probe-1/~[work~]/cfg/**'), canary],
  ['after the fix: no longer a class', e('d/tmp/claude-glob-probe-1/~[work~]/cfg/**'), 'd/tmp/claude-glob-probe-1/w/cfg/canary.txt'],
  ['\\* is a literal star', e('d/a~*b/**'), 'd/a*b/x'],
  ['\\* matches no other name', e('d/a~*b/**'), 'd/axxb/x'],
  ['{a,b} needs no escape, \\{ \\} harmless', e('d/~{a,b~}/**'), 'd/{a,b}/x'],
  ['\\! mid-pattern is a literal', e('d/x~!y/**'), 'd/x!y/x'],
  ['parentheses need no escape', 'd/Finance (2024)/**', 'd/Finance (2024)/x'],
  ['\\? matches nothing (kept as ?)', e('d/a~?b/**'), 'd/a?b/x'],
  ['? stays a one-character wildcard', 'd/a?b/**', 'd/a?b/x'],
];
for (const [what, pattern, path] of rows) {
  console.log(`${String(ignore().add(pattern).ignores(path)).padEnd(6)} ${what}: ${JSON.stringify(pattern)} vs ${path}`);
}
