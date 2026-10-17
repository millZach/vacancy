# Vacancy

See where your disk space went, get it back, and find out what changed.

Vacancy is a desktop app for macOS and Windows. It maps your drive, proposes how to get space back, runs the plan you approve, and finishes with a report of everything it did.

Vacancy is in early development. Releases before 1.0 are previews, and each release's notes say what works so far.

## What Vacancy does

- **Maps your drive.** You see where the space goes as a map you can explore.
- **Proposes cleanups.** Built-in rules find known junk offline: caches, temp files, old installers, build folders, and leftovers from Docker, Xcode and package managers. Each proposal shows its size, why it is safe, and how risky it is. Duplicate detection is still planned.
- **AI assistance is deferred.** Claude Code and Codex integrations remain unavailable until Vacancy can enforce metadata-only access. Unknown-folder explanations and AI reorganization are not active features.
- **Runs only what you approve.** You pick the items and confirm once, then watch the space come back.
- **Shows what it did.** The run ends with a report: space freed, what was deleted, what was moved, and what was skipped and why.

## What Vacancy never touches

Deleting is permanent: nothing goes to the Trash or the Recycle Bin. These rules decide what Vacancy can touch:

- **Only what you confirmed.** The confirm screen lists every item. Vacancy acts on those items and nothing else.
- **Protected places stay off-limits.** These are system folders, apps, the root of your home folder, cloud-sync folders (iCloud Drive, OneDrive, Dropbox and others), your Photos library, Time Machine, and anything outside the folders Vacancy is allowed to clean. Vacancy shows them as locked, and no setting, rule or AI suggestion can unlock them.
- **Changed items are skipped.** Before each action, Vacancy checks the item again. It skips anything that changed since the scan, turned into a link, or is in use.
- **Links are never followed.** Vacancy never follows a symlink or a Windows junction while it scans or deletes, so a link can't lead it into your documents.
- **Metadata-only AI is the acceptance boundary.** Any future integration must send only names, sizes, counts and dates, never file contents. It may propose items for review but cannot execute them. The current integrations stay disabled because this boundary is not enforceable with the available CLIs.
- **Journaled moves and Undo are planned.** Reorganization is not available yet. Before it can ship, Vacancy must record each move before it happens and let the report put it back without overwriting or deleting files.

## Download and install

Download the installer for your computer from the [latest release](https://github.com/millZach/vacancy/releases/latest).

| Your computer | File |
| --- | --- |
| Windows 10 or 11 | `Vacancy-<version>-windows-x64-setup.exe` |
| Mac with Apple silicon or Intel, macOS 11 or later | `Vacancy-<version>-macos-universal.dmg` |

The builds aren't signed yet (see [Signing](#signing)), so Windows and macOS warn you the first time you open them.

### Windows

1. Run `Vacancy-<version>-windows-x64-setup.exe`.
2. If Windows shows "Windows protected your PC" (SmartScreen), click **More info**, then **Run anyway**.
3. Follow the installer. It installs Vacancy for your user account only and doesn't ask for admin rights.

To uninstall Vacancy, go to **Settings > Apps > Installed apps**, find Vacancy and click **Uninstall**.

Some releases also include `Vacancy-<version>-windows-x64.msi`. It installs Vacancy for every user on the PC, so it needs admin rights. It is meant for managed installs. Most people should use the setup `.exe`.

### macOS

1. Open `Vacancy-<version>-macos-universal.dmg` and drag **Vacancy** into **Applications**.
2. In **Applications**, right-click (or Control-click) **Vacancy**, choose **Open**, then click **Open** again.
3. On macOS 15 Sequoia and later, the right-click **Open** option is gone. Try to open Vacancy once, then go to **System Settings > Privacy & Security**, scroll down to the message about Vacancy, and click **Open Anyway**.

You can also clear the download warning from Terminal:

```sh
xattr -dr com.apple.quarantine /Applications/Vacancy.app
```

To uninstall Vacancy, drag it from **Applications** to the Trash.

### Check your download

Each release includes `SHA256SUMS.txt` with a checksum for every file. To check a download:

- Windows (PowerShell): `Get-FileHash .\Vacancy-<version>-windows-x64-setup.exe`, then compare the hash with the one in `SHA256SUMS.txt`.
- macOS (Terminal, in the folder with the download): `shasum -a 256 -c SHA256SUMS.txt --ignore-missing`.

## Signing

The Windows builds aren't code-signed. The macOS builds are ad-hoc signed, which Apple silicon Macs need to run them at all, but they aren't signed with an Apple Developer ID or notarized by Apple. That is why both systems warn you the first time you open Vacancy.

Proper signing is planned as a follow-up: a Windows code-signing certificate, and Developer ID signing with notarization on macOS. Both cost money, so they wait on a decision from Zach.

## Build from source

You need Node.js 24 and stable Rust, plus [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/) for your system. Then:

```sh
npm ci
npm run tauri dev      # run the app
npm run tauri build    # build the installers into target/release/bundle/
```

[AGENTS.md](AGENTS.md) covers the layout, the tests and how to prove a change.

## Releasing

The version is set in one place: `version` under `[workspace.package]` in `Cargo.toml`. The app, the installers and the release all use it.

1. Set the new version in `Cargo.toml` and run `cargo check` so that `Cargo.lock` picks it up. Merge that change to `main`.
2. Tag the merge commit `v<version>` (for example `v0.2.0`) and push the tag.
3. The [Release workflow](.github/workflows/release.yml) checks that the tag matches the version. It then builds the Windows installers and the universal macOS `.dmg` and attaches them, with `SHA256SUMS.txt`, to a **draft** release with generated notes.
4. Install the build from the draft on both systems, then publish the draft.

A version with a pre-release part, such as `0.2.0-beta.1`, is marked as a pre-release. It ships without the `.msi`, because the MSI format only accepts a numeric pre-release (`0.2.0-1`).
