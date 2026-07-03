# Layout Component Reference Library

This library contains standard React + Tailwind CSS components optimized for dense layouts on both mobile touch screens and desktop/embedded dashboard screens.

---

## 1. High-Density Diagnostic Card (Touch/Mobile View)
Designed for mobile touch views where space is limited and secondary details are progressively disclosed on tap.

### Key Enhancements & Invariants:
* **Tailwind Correction**: Uses standard `border-l-4` instead of invalid `border-l-3`.
* **Hoisted Map Calculations**: Sparkline calculations (`Math.max` and `Math.min`) are hoisted outside the `.map()` block to avoid $O(n^2)$ recalculations.
* **Accessibility**: Includes `aria-expanded` and an `aria-label` or `sr-only` label for screen readers on the collapsible button.
* **Pointer Adaptability**: Removed touch-incompatible hover interactions. Touch targets are scaled to a minimum height of `48px`.

```tsx
import React, { useState } from "react";
import { ChevronDown, AlertTriangle, Activity } from "lucide-react";

interface DiagnosticCardProps {
  label: string;
  value: string | number;
  unit: string;
  status: "ok" | "warn" | "crit";
  pidAddress: string;
  updateFrequency: string;
  historyData: number[]; // Sparkline telemetry history
}

export const DiagnosticCard: React.FC<DiagnosticCardProps> = ({
  label,
  value,
  unit,
  status,
  pidAddress,
  updateFrequency,
  historyData,
}) => {
  const [isExpanded, setIsExpanded] = useState(false);

  const statusColors = {
    ok: { text: "text-emerald-400", border: "border-l-emerald-500", bg: "bg-emerald-500/10" },
    warn: { text: "text-amber-400", border: "border-l-amber-500", bg: "bg-amber-500/10" },
    crit: { text: "text-red-400", border: "border-l-red-500", bg: "bg-red-500/10" },
  };

  // Hoist calculations out of map to ensure O(n) rendering complexity
  const hasHistory = historyData && historyData.length > 0;
  const maxVal = hasHistory ? Math.max(...historyData) : 0;
  const minVal = hasHistory ? Math.min(...historyData) : 0;
  const valRange = maxVal - minVal;

  return (
    <div className={`bg-zinc-900 border border-zinc-800 border-l-4 ${statusColors[status].border} rounded overflow-hidden`}>
      {/* Touch Target (Minimum Height >= 48px) */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        aria-expanded={isExpanded}
        aria-label={`Toggle details for ${label}, status: ${status}`}
        className="w-full min-h-[48px] flex items-center justify-between p-3 cursor-pointer text-left active:bg-zinc-800/50"
      >
        <div className="flex items-center gap-2">
          {status !== "ok" && (
            <AlertTriangle className={`w-4 h-4 ${statusColors[status].text}`} aria-hidden="true" />
          )}
          <span className="text-sm font-semibold text-zinc-100">{label}</span>
        </div>
        <div className="flex items-center gap-3">
          <div className="text-right">
            <span className="text-lg font-bold font-mono text-zinc-100">{value}</span>
            <span className="text-[10px] font-mono text-zinc-400 ml-1">{unit}</span>
          </div>
          <ChevronDown className={`w-4 h-4 text-zinc-500 transition-transform duration-150 ${isExpanded ? "rotate-180" : ""}`} />
        </div>
      </button>

      {/* Expanded Detail Panel (Lazy Render) */}
      {isExpanded && (
        <div className="border-t border-zinc-800/80 bg-zinc-950 p-3 space-y-3">
          <div className="grid grid-cols-2 gap-2 text-xs font-mono">
            <div className="flex justify-between border-b border-zinc-900 pb-1">
              <span className="text-zinc-400">PID:</span>
              <span className="text-zinc-300">{pidAddress}</span>
            </div>
            <div className="flex justify-between border-b border-zinc-900 pb-1">
              <span className="text-zinc-400">RATE:</span>
              <span className="text-zinc-300">{updateFrequency}</span>
            </div>
          </div>

          {hasHistory && (
            <div className="h-10 flex items-end gap-[2px] pt-1" aria-label="10s sparkline telemetry trend">
              {historyData.map((val, idx) => {
                const heightPercent = valRange === 0 ? 50 : ((val - minVal) / valRange) * 100;
                return (
                  <div
                    key={idx}
                    className="flex-1 bg-zinc-700 active:bg-emerald-400"
                    style={{ height: `${Math.max(4, heightPercent)}%` }}
                  />
                );
              })}
            </div>
          )}
          <div className="flex justify-between text-[9px] font-mono text-zinc-500">
            <span>HISTORY (10s)</span>
            <span className="flex items-center gap-1">
              <Activity className="w-3 h-3 text-emerald-500" /> ACTIVE
            </span>
          </div>
        </div>
      )}
    </div>
  );
};
```

---

## 2. Telemetry Grid Card (Dashboard / Desktop View)
Designed for in-vehicle dashboards or desktop interfaces where high density is prioritized and values must stay visible rather than hidden behind tabs.

### Key Enhancements & Invariants:
* **Accessibility**: Uses `role="table"` semantics (not `role="grid"`) because the data is static and read-only. `role="grid"` advertises spreadsheet-style keyboard navigation that this component does not implement, which misleads assistive tech. Cells use `role="cell"` (a `grid` would use `gridcell`). Prefer a native `<table>` where CSS layout permits.
* **No Inline Interactive Gestures**: Employs static display rendering appropriate for non-touch view modes.

```tsx
import React from "react";

interface GridRow {
  id: string;
  name: string;
  rawVal: string;
  scaledVal: number | string;
  unit: string;
  status: "ok" | "fail";
}

interface TelemetryGridProps {
  title: string;
  subtitle?: string;
  rows: GridRow[];
}

export const TelemetryGrid: React.FC<TelemetryGridProps> = ({ title, subtitle, rows }) => {
  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded p-3" role="region" aria-label={title}>
      {/* Header Block */}
      <div className="mb-2">
        <h3 className="text-[13px] font-bold text-zinc-200 uppercase tracking-wide">{title}</h3>
        {subtitle && <p className="text-[10px] text-zinc-500 leading-tight">{subtitle}</p>}
      </div>

      {/* Table Container (static, read-only tabular data) */}
      <div className="w-full text-left font-mono" role="table">
        <div className="grid grid-cols-12 gap-1 text-[10px] text-zinc-400 border-b border-zinc-800 pb-1 font-bold" role="row">
          <span className="col-span-3" role="columnheader">ID</span>
          <span className="col-span-5" role="columnheader">PARAMETER</span>
          <span className="col-span-2 text-right" role="columnheader">RAW</span>
          <span className="col-span-2 text-right" role="columnheader">VAL</span>
        </div>

        <div className="divide-y divide-zinc-800/40 text-xs" role="rowgroup">
          {rows.map((row) => (
            <div key={row.id} className="grid grid-cols-12 gap-1 py-1.5 items-center" role="row">
              <div className="col-span-3 font-semibold text-zinc-300" role="cell">{row.id}</div>
              <div className="col-span-5 text-zinc-400 truncate pr-1" role="cell">{row.name}</div>
              <div className="col-span-2 text-right text-zinc-400 text-[11px]" role="cell">{row.rawVal}</div>
              <div className="col-span-2 text-right font-bold" role="cell">
                <span className={row.status === "fail" ? "text-red-400" : "text-emerald-400"}>
                  {row.scaledVal}
                </span>
                <span className="text-[9px] font-normal text-zinc-500 ml-0.5">{row.unit}</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
```
