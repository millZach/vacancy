// `npm run tauri -- <command> [args]`: the Tauri CLI, with fast scan's reader (`vacancy-index-reader`) built first
// on Windows, because the app starts it from its own folder.
//
// - `build`: builds the reader in release, copies it to src-tauri/binaries/ under the name Tauri bundles beside the
//   app (vacancy-index-reader-<target triple>.exe), and passes src-tauri/tauri.reader.conf.json so the installers
//   carry it. The reader's SHA-256 goes to the app's build as VACANCY_READER_SHA256: the app only starts the reader
//   it came with (see ADR 0007).
// - `dev`: builds the reader in debug and copies it beside the dev app (target/debug), where a debug app looks for it.
// The reader is built with the C runtime linked in (its own target folder, target/reader), so it imports only
// Windows' own DLLs.
// - Anything else, and every command on macOS (which has no fast scan), goes straight to the CLI.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const command = args[0];
const windows = process.platform === 'win32';

function run(cmd, cmdArgs, env = process.env) {
  const r = spawnSync(cmd, cmdArgs, { cwd: root, stdio: 'inherit', env, shell: false });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}

/** The target triple to build for: `--target <triple>` if given, else the host's. */
function triple() {
  const i = args.indexOf('--target');
  if (i >= 0 && args[i + 1]) return args[i + 1];
  const out = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
  const host = /^host: (\S+)$/m.exec(out.stdout ?? '');
  if (!host) throw new Error('rustc -vV gave no host triple');
  return host[1];
}

const env = { ...process.env };
const extra = [];
if (windows && command === 'build') {
  const target = triple();
  const explicit = args.includes('--target');
  // The C runtime linked in: the reader imports only Windows' own DLLs, which load from System32 alone, so no DLL
  // beside it (in a folder the user can write to) is ever loaded. Its own target folder keeps the app's build cache.
  const readerEnv = { ...process.env, RUSTFLAGS: '-C target-feature=+crt-static', CARGO_TARGET_DIR: join(root, 'target', 'reader') };
  // Encoded flags would win over RUSTFLAGS and drop +crt-static (the reader then refuses to compile in release).
  delete readerEnv.CARGO_ENCODED_RUSTFLAGS;
  run('cargo', ['build', '--release', '--locked', '-p', 'vacancy-index', '--bin', 'vacancy-index-reader', ...(explicit ? ['--target', target] : [])], readerEnv);
  const built = join(root, 'target', 'reader', ...(explicit ? [target] : []), 'release', 'vacancy-index-reader.exe');
  const bundled = join(root, 'src-tauri', 'binaries', `vacancy-index-reader-${target}.exe`);
  mkdirSync(dirname(bundled), { recursive: true });
  copyFileSync(built, bundled);
  env.VACANCY_READER_SHA256 = createHash('sha256').update(readFileSync(bundled)).digest('hex');
  console.log(`tauri.mjs: bundling ${bundled} (sha256 ${env.VACANCY_READER_SHA256})`);
  extra.push('--config', join('src-tauri', 'tauri.reader.conf.json'));
} else if (windows && command === 'dev') {
  // Beside the dev app, where a debug app looks for it (the C runtime linked in, as in a release).
  run('cargo', ['build', '-p', 'vacancy-index', '--bin', 'vacancy-index-reader'], {
    ...process.env,
    RUSTFLAGS: '-C target-feature=+crt-static',
    CARGO_TARGET_DIR: join(root, 'target', 'reader'),
  });
  mkdirSync(join(root, 'target', 'debug'), { recursive: true });
  copyFileSync(
    join(root, 'target', 'reader', 'debug', 'vacancy-index-reader.exe'),
    join(root, 'target', 'debug', 'vacancy-index-reader.exe'),
  );
}

const cli = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
// Right after the command: anything after a `--` goes to cargo.
run(process.execPath, [cli, args[0], ...extra, ...args.slice(1)].filter((a) => a !== undefined), env);
