# Codex Handoff — Design & Review Session: obd2-gui against the Layout Design System

**Prepared:** 2026-07-03 · **Target:** the real `apps/obd2-gui` app · **Mode:** design *and* review (both deliverables)

You are Codex, running a working session in this repo. Deliver two things: (1) representative
dashboard UI built **with** the design system, and (2) a written review of where the system
**held up or broke** when applied to a real, data-heavy diagnostic app. This is a
*reconciliation* job, not greenfield — a substantial dashboard already exists.

---

## 0. TL;DR of what to do

1. Read the three design-system files (this dir) and the existing app (`src/App.tsx`, `src/types.ts`).
2. Audit the existing UI against the system; write findings.
3. Build 2–3 representative screens/components that apply the system to *real* app data.
4. Verify: `tsc` + build clean, render in-browser, screenshot with Playwright, check contrast.
5. Write the review doc (`REVIEW.md` in this dir) — gaps, contradictions, recommendations.
6. Do **not** silently mutate the whole app. Work on a branch; keep existing tabs functional.

---

## 1. Environment ground truth (verified, do not assume otherwise)

| Fact | Value | Implication |
|---|---|---|
| App type | **Tauri 2 desktop app** (React 18 + Vite 8 + TS 5.6) | Desktop/embedded → the **density-wins** branch of the system applies, *not* mobile disclosure. |
| CSS | **Tailwind CSS v4** via `@tailwindcss/vite`. `@import "tailwindcss"` in `src/styles.css`. **No `tailwind.config.js`** — that's the v4 CSS-first model, not a mistake. | Customize via `@theme` in CSS, not a JS config. Default `zinc` palette is present. In v4 the default `border` color is `currentColor`, so always name border colors explicitly (the components already do). |
| Icons | `lucide-react@0.468` already installed | The design-system components import from `lucide-react` — no new dep needed. |
| Font | Page font is **Inter** (`:root` in `styles.css`); page bg is `#090b0d`, text `#f2f5f2`. | **No monospace stack is defined.** The system leans on `font-mono` for telemetry column alignment — `font-mono` currently falls back to the browser default. Add a mono/tabular stack (or use `tabular-nums`) or the alignment the system promises won't materialize. |
| Existing UI | **`src/App.tsx` is 1,926 lines** — a real dashboard with tabs (`overview / active / diagnostics / raw / settings` + dynamic `cap:*` capability tabs), unit modes (`us`/`metric`), session modes (`live`/`recording`/`replay`). | Reconcile with this. Do not design in a vacuum; the system must fit what's here. |
| Test infra | `@playwright/test` installed | Use it to screenshot and visually verify your screens. |
| Dev server | `npm run dev` → Vite on `127.0.0.1` | For browser verification. `npm run build` runs `tsc && vite build`. |

## 2. The design system (canonical source + snapshot)

- **Canonical source of truth:** `~/.gemini/config/skills/mobile-layout-design/` (a Gemini skill, outside this repo). If your sandbox can't read it, use the in-repo snapshot below.
- **In-repo snapshot (read these):**
  - `design-system-rules.md` — the rules: density-vs-disclosure, 2-level IA, 4px-baseline grid, typography scale, text-duplication prevention, a11y floors, performance checkpoints.
  - `components.md` — reference React+Tailwind components: `DiagnosticCard` (collapsible, mobile) and `TelemetryGrid` (dense, desktop).

**Any change you recommend to the system must be noted for back-porting to the canonical
`~/.gemini` copy — the in-repo files are a dated snapshot, not the master.**

### The system in one paragraph
Flat IA (max 2 levels). For embedded/desktop dashboards, **density wins**: critical telemetry
(RPM, coolant, speed, DTCs) is always visible; progressive disclosure is reserved for
*secondary* detail (raw hex, history graphs, evidence, config). Spacing/line-height snap to a
**4px-baseline grid**. Hierarchy via a contrast-opacity scale. A11y floors: 48px touch targets,
**≥4.5:1 contrast for meaning-carrying text** (only decorative metadata may drop to 3:1),
`aria-label` on icon-only controls, correct `role="table"` (never `role="grid"`) for static data.
Perf: no `backdrop-filter: blur()`, fixed chart heights, lazy-mount collapsed content.

## 3. Known reconciliation points — resolve these explicitly, don't paper over them

These are the seams where the system meets reality. Each needs a decision + a note in `REVIEW.md`:

1. **State vocabulary mismatch.** The system's `DiagnosticCard` assumes `status: "ok"|"warn"|"crit"`.
   The real model (`src/types.ts`) is richer: `StateKind = ok|warn|crit|muted`, and
   `SignalRuntimeState = ok|waiting|cached|unsupported|error`. The 3-state color map does **not**
   cover `waiting/cached/unsupported/error/muted`. Extend the status→color mapping to the real
   states, and define what each looks like (e.g., `unsupported`/`muted` = low-contrast gray,
   `waiting`/`cached` = a distinct non-alarm treatment). This is finding #1 for the review.
2. **Palette collision.** The app already has a visual language: `#090b0d` bg, `#f2f5f2` text,
   Inter, a subtle top gradient. The system prescribes the `zinc` scale. **Do not introduce a
   second parallel palette.** Either (a) map the system's zinc tokens onto the app's existing
   colors via `@theme` in `styles.css`, or (b) adopt zinc app-wide — and say which, and why.
3. **Density path, not the mobile card.** Because this is desktop, lead with `TelemetryGrid`
   (values always visible). Use the collapsible `DiagnosticCard` pattern only for the app's
   existing *secondary* surfaces (`raw` tab, evidence/provenance, per-signal history) — which
   map naturally to the system's "disclosure scope."
4. **Monospace/tabular figures.** Define a mono stack or apply `tabular-nums` so numeric columns
   actually align. Right now `font-mono` is unstyled fallback.
5. **Real data richness the components don't model.** Signals carry `confidence`
   (`Candidate/LiveObserved/Community/Verified/Rejected`), `provenance[]`, `evidence_policy`,
   `composition` (scalar/pair/table_row/derived). The reference components have no slot for
   provenance/confidence. Decide how the system surfaces trust/provenance — this is likely the
   biggest genuine *gap* in the system for this domain. Flag it.

## 4. Session plan (phased — verify at each gate)

**Phase A — Audit (write, don't edit yet).** Read `App.tsx` + `types.ts` fully. Produce, in
`REVIEW.md`, a table of every place the current UI already agrees with or violates the system
(IA depth, spacing consistency, contrast, hidden-vs-visible telemetry, ARIA correctness).

**Phase B — Design.** Pick 2–3 representative surfaces to (re)build with the system. Suggested,
in priority order: (1) the **overview** telemetry grid (density path), (2) an expandable
**signal/diagnostic card** for the secondary/raw surface (disclosure path, with provenance +
one of the extra states), (3) the **DTC list**. Use *real* shapes from `types.ts` and real
fixtures from `src/mockData.ts` — no invented data.

**Phase C — Build.** Implement as new components under `src/components/` (create the dir).
Wire at least one into the live app behind the existing tab structure so it renders with real
mock data. **Keep all existing tabs working** — do not delete or break current functionality.

**Phase D — Verify (mandatory gates, show evidence):**
- `npm run build` (i.e. `tsc && vite build`) exits 0.
- `npm run dev`, load the app, and use Playwright to screenshot the new surfaces (light path: the
  app is dark-only via `color-scheme: dark`). Attach/save screenshots to `docs/design-system/`.
- **Contrast check:** for every text/background pair you introduce, compute the WCAG ratio against
  the *actual* rendered hex (remember the page bg is `#090b0d`, and `@theme` may remap zinc).
  Confirm meaning-carrying text ≥ 4.5:1. Report the numbers, don't assert "looks fine."
- No `backdrop-filter`, no `role="grid"` on static data, no `border-l-3` (invalid in Tailwind).

**Phase E — Review.** Write `docs/design-system/REVIEW.md`:
- Verdict on the three original questions: does it solve the problem / meet requirements /
  deliver working layout — *now judged against a real app*, not in the abstract.
- The reconciliation decisions (§3) and their rationale.
- **Gaps found:** missing tokens, states, or components (provenance/confidence surfacing is the
  expected big one). Propose concrete additions to back-port to the canonical skill.
- Any contradiction or dead guidance you hit.
- A short "would I ship this system for this app?" recommendation.

## 5. Guardrails

- **Branch first:** `git checkout -b design/obd2-dashboard-system`. The working tree already has
  uncommitted changes on `docs/multi-oem-protocol-architecture`; keep your work isolated and do
  not commit unrelated modified files.
- Additive over destructive: new components + minimal wiring. Don't rewrite the 1,926-line
  `App.tsx` wholesale.
- No new runtime dependencies without justification (lucide + tailwind v4 are already here).
- This is a **Rust-workspace monorepo**; stay within `apps/obd2-gui` unless a change genuinely
  requires touching `src-tauri` (it shouldn't for pure UI).
- Surface open questions in `REVIEW.md` rather than guessing on ambiguous product decisions
  (e.g., "should `unsupported` signals be hidden or shown grayed?").

## 6. Deliverables checklist

- [ ] `docs/design-system/REVIEW.md` — audit + reconciliation decisions + gaps + verdict.
- [ ] `src/components/*` — 2–3 real components using real `types.ts` shapes + `mockData` fixtures.
- [ ] At least one component wired into the live app and rendering.
- [ ] Screenshots of the new surfaces in `docs/design-system/`.
- [ ] Contrast numbers reported for introduced color pairs.
- [ ] Clean `npm run build`.
- [ ] A note listing changes to back-port to the canonical `~/.gemini` design-system source.
