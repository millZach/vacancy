# Tools and system advice · throwaway prototype

Question: How should instruction-only Windows, Docker, Xcode and Homebrew reminders fit the existing Atlas plan?

Run `npm run prototype:tools` and open http://127.0.0.1:1434/?variant=A. Use the floating arrows or keyboard arrows for A/B/C, and Windows/macOS to inspect platform guidance. Expand a title, tick a reminder, review it and simulate the zero-change recap. A is recommended: it extends the established Left for you pattern. B is a grouped guidance notebook; C keeps guidance in a drawer over the map. State is in memory, survives variant switches and appears on the switcher. Platform-specific reminder selections remain in memory and return on switching back.

All drive data is synthetic. The prototype imports the real Shell, map, row controls and design tokens. It never calls the app bridge, probes installed tools, runs commands, launches apps or changes files. Vendor documentation links open only when clicked. Cleanup rows are inert context; all reminders contribute zero bytes. No production source files are changed.

Dependencies: this worktree's root `node_modules` is a prototype-only Windows junction to `D:\Memory Cleanup\.zstack\wt\proto-advisor\node_modules`, created without installing or modifying that dependency tree. Remove only the junction itself if cleaning up; never recursively delete through it.

`npm run prototype:tools:check` checks TypeScript, `npm run prototype:tools:build` builds this isolated route. Build/check/browser capture must run under the shared `Invoke-HeavyJob.ps1` wrapper. The browser proof script starts its own temporary Vite server and closes it in `finally`; root may host a separate review server after proof.

Sources are linked next to each guide: Microsoft Storage and Delivery Optimization support, Docker Desktop Images and build-cache garbage collection, Apple Xcode components, and the Homebrew manpage. They describe vendor-owned review flows, not measured cleanup findings. No claim is made that error reports or unused runtimes were found.

Static review delivery: build uses relative asset paths (`base: './'`) so `dist` works beneath the private `/vacancy-tools/` checkpoint path. Append `?review=1&variant=A` to intentionally display the comparison toolbar in the built throwaway route. Without that explicit flag the toolbar remains hidden in production builds; dev always shows it. This exception is scoped to this isolated prototype, not the app. The final browser proof serves built output with `review=1`; root verifies the exact private HTTPS checkpoint URL separately.

No user verdict yet. Do not promote this prototype or push a tag until Zach chooses a design.
