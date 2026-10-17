# AI status placement — throwaway prototype

Question: where should Vacancy explain that the installed AI apps cannot be used under its metadata-only privacy promise?

Run from the repository root: `npm run prototype:advisor` (after `npm ci`). Open `http://127.0.0.1:1433/?variant=A`.

- A: compact menu anchored to the existing header badge.
- B: settings panel along the right edge.
- C: focused dialog in the existing app.

Left/right arrows or the floating prototype bar cycle the variants and update the URL. A variant URL survives reload. Escape, the close button or the surrounding area dismisses a treatment; the header badge and scan note reopen it. Use `?fixture=macos&variant=B` for the Mac fixture, or `&open=0` to start closed. Scan opens the real Atlas UI with mock data; all actions are simulated. No AI, credentials, OS operations or external services are used.

Both provider rows are synthetic installed-but-unsupported states. AI controls are disabled in every treatment. No API-key strategy is assumed. The host's macOS fixture uses simulated Full Disk Access granted to keep the design question focused. This prototype is not a production entry point and does not modify `src/`. Its floating switcher is enabled only in Vite's explicit `prototype` mode.

`npm run prototype:advisor:check` checks TypeScript; `npm run prototype:advisor:build` creates the standalone preview in `prototypes/advisor-status/dist/`. Do not promote this code to production. Capture a tag only after Zach chooses a treatment.
