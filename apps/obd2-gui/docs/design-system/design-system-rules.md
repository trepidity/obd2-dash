---
name: mobile-layout-design
description: Rules, design tokens, and components for building clean, uncluttered, and highly readable interfaces (mobile touch layouts and desktop/embedded dashboards) with flat information architectures.
---

# Mobile & Dashboard Layout Design Custom Skill

Use this skill when designing, building, or refactoring UI layouts (specifically for mobile applications or dense desktop/embedded dashboards such as telemetry systems) to ensure human-readability, high data density without clutter, accessibility, and high performance.

---

## 1. The Core Architectural Rule: Density vs. Disclosure

Depending on the target device type, apply the correct structural rule to determine when values must be visible:

1. **Embedded/Desktop Dashboards (e.g. OBD-II Dashboards, Vehicle Panels)**:
   * **Density Wins**: Immediate visibility of critical parameters (e.g. Engine RPM, Coolant Temp, Speed, DTC Codes) is mandatory. Do *not* hide primary telemetry or warnings behind tabs or expanders.
   * **Disclosure Scope**: Reserve progressive disclosure (collapsible accordions, modals, bottom sheets) strictly for *secondary details* (e.g., raw HEX packet dumps, long-term history graphs, alert configuration inputs, or troubleshooting procedures).

2. **Consumer Mobile/Touch Applications**:
   * **Disclosure Wins**: Group complex data into clean, collapsible container summaries (cards) to keep view lengths manageable and limit vertical scrolling.

---

## 2. Information Architecture (IA) Guidelines (Max 2 Levels Deep)

*   **Level 1: Navigation Root**: Use a persistent bottom tab bar (3 to 5 items max) on mobile touch layouts. Use static sidebar navigation or split panels on desktop layouts.
*   **Level 2: Segmented Controls**: Partition content *within* a Level 1 view using inline toggle selectors. Avoid pushing the user to secondary pages for navigation.
*   **Action Overlays**: Use modal sheets that slide up from the bottom (mobile) or centered popup dialogs (desktop) for editing thresholds, input fields, or settings to maintain visual context.

---

## 3. Spacing & Typography: The 4px-Baseline Grid

Avoid arbitrary margins, paddings, and line heights. Always snap sizes to increments of a **4px-baseline grid**.

### 3.1 Spacing Scale
Use these baseline-grid spacing steps for alignment:

*   `space-xxs` (2px): Micro-borders, tiny icon-to-text gaps.
*   `space-xs` (4px): Inner badge/tag padding, vertical label-to-value gaps.
*   `space-sm` (8px): Card inner padding, list item margins, element-to-element gaps.
*   `space-md` (12px): Grid gutters, dense card margins.
*   `space-lg` (16px): Viewport gutters, primary section padding.
*   `space-xl` (24px): Major layout block division padding.

### 3.2 Modular Typography & Grid-Snapped Line-Heights
Line-heights must snap to the 4px baseline grid to prevent vertical layout shifts:

*   **Monospace Data/Unit**: `text-[10px] leading-3 font-mono` (12px line height).
*   **Metadata / Captions**: `text-xs leading-4` (16px line height).
*   **Standard Body / Key Values**: `text-sm leading-5` (20px line height).
*   **Component Headers**: `text-[15px] leading-5 font-semibold` (20px line height).
*   **Section Headers**: `text-lg leading-6 font-bold` (24px line height).
*   **Hero Telemetry Readings**: `text-3xl leading-8 font-extrabold font-mono` (32px line height).

---

## 4. Preventing Text Duplication & Visual Noise

1.  **Direct Labeling & Unified Headings**: Place units in grid headers or card titles. Do not repeat units (e.g. `MPH`, `°C`) in every list row.
2.  **No Redundant Category Labels**: If a card lives under the "Workouts" section, name it "Deadlift" rather than "Deadlift Workout."
3.  **Contrast Opacity Scale**: Establish content hierarchy using text color opacity rather than shifting font weights or sizes:
    *   *Primary Value*: High contrast (e.g., `#FFFFFF` / `rgba(255,255,255,0.95)`).
    *   *Data Labels / Units*: Muted contrast (e.g., `#A1A1AA` / `rgba(255,255,255,0.60)`).
    *   *Timestamps / Metadata*: Low contrast (e.g., `#71717A` / `rgba(255,255,255,0.40)`).

---

## 5. Accessibility Baseline (A11y)

1.  **Touch Targets**: Touch targets on mobile must have a minimum physical size of `48px` in height and width.
2.  **Contrast Ratios**: Primary values and any text that carries meaning (labels, metrics, parameter names, raw/scaled readings) must maintain a contrast ratio of at least **4.5:1** against their background. Only the lowest de-emphasized tier from §4 — purely decorative or duplicative metadata such as timestamps, unit suffixes, and section captions — may drop to **3:1**, and never below. This is the reconciliation of the opacity hierarchy in §4: hierarchy is expressed by stepping *within* these floors, not beneath them. (Note: `text-zinc-500` on a `zinc-900` background is ≈3.6:1 — acceptable only for that decorative tier, not for data.)
3.  **Screen Readers**: Icon-only actions (like deletion trash cans or expansion arrows) must feature explicit `aria-label` tags or nested hidden tags with screen reader classes (`sr-only`) to state their function.
4.  **No Touch-Incompatible Interactions**: Avoid relying on `:hover` states to reveal critical info or actions on mobile interfaces, as hover does not translate to touch screen gestures.
5.  **Correct ARIA for Static Data**: Do not use `role="grid"`/`gridcell` for read-only tables — that role promises keyboard grid navigation you must otherwise implement. Use a native `<table>`, or `role="table"`/`row`/`columnheader`/`cell` for static tabular telemetry.

---

## 6. Implementation & Component Library

Refer to the [Layout Component Reference Library](references/components.md) for standard React + Tailwind CSS code implementations of:
*   High-Density Diagnostic Cards (collapsible, lazy rendering).
*   Telemetry Grid Cards (highly dense, structured, accessible).

---

## 7. Performance Checkpoints

1.  **Avoid Backdrop Filters**: Do *not* apply `backdrop-filter: blur()`. It forces expensive multi-pass frame buffer copies that drastically reduce rendering speeds on embedded screens. Use opaque background colors (`bg-zinc-950`) instead.
2.  **Fixed Grid Heights**: Specify fixed heights or aspect ratios for graphs and charts to prevent downstream layout shifts when they finish loading.
3.  **Prune DOM Node Size**: Lazily mount collapsible components (`isExpanded && <Details />`) so the browser avoids rendering hidden nodes.
