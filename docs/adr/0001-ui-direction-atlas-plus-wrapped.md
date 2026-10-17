# 0001: UI direction is "Atlas + Wrapped"

Date: 2026-10-16. Status: accepted (Zach).

We prototyped three structurally different UIs for the whole flow (A: a treemap you explore, B: a conversational feed ending in a receipt, C: a dense ledger under a neon sign ending in story slides). Zach picked a combination, prototyped as variant D:

- **Scan, overview, plan and run happen on the map** (A). The disk is a treemap; reclaimable areas glow; a plan tray collects proposals; the run drains cleared tiles.
- **The finish is C's "Wrapped" story slides**, restyled to A's look, which then **hand off to A's before/after map summary** as the resting screen.
- **The scan screen has a living dot field**: ambient motion, and a water-droplet ripple where you click (his reference: the new-thread background in Claude's desktop app).

The prototype is kept as tag `prototype/vacancy-ui` (variant D in `prototypes/vacancy-ui/src/variants/D`). It is the visual reference. The app's UI is a proper rewrite of its structure (components, state, data flow, error handling), not a copy of prototype code. The one thing kept as-is is the maths that makes the look, such as the dot field's ripple, because matching that look exactly is the point.
