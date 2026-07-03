import { Activity, AlertTriangle, Fuel, Gauge, ListTree, Radio, Table2 } from "lucide-react";

import type {
  CapabilitySection,
  CapabilitySectionCategory,
  DiagnosticSnapshot,
  SignalSnapshot,
  StateKind,
} from "../types";
import { EvidenceDisclosure } from "./EvidenceDisclosure";
import {
  cx,
  diagnosticLabelClassName,
  diagnosticSecondaryTextClassName,
  isOperationalSignal,
  runtimeStateToken,
  stateKindToken,
  trustToken,
} from "./diagnosticTokens";

export type TelemetryUnitMode = "us" | "metric";

type PairSignal = SignalSnapshot & {
  composition: Extract<SignalSnapshot["composition"], { kind: "pair" }>;
};

type TableRowSignal = SignalSnapshot & {
  composition: Extract<SignalSnapshot["composition"], { kind: "table_row" }>;
};

interface TelemetryBoardProps {
  snapshot: DiagnosticSnapshot;
  unitMode: TelemetryUnitMode;
}

interface CriticalTile {
  key: string;
  label: string;
  value: string;
  state: StateKind;
}

const pressureRatio = 6.894757;

function formatSigned(value: number, digits: number): string {
  const formatted = value.toFixed(digits);
  return value > 0 ? `+${formatted}` : formatted;
}

function psiToKpa(psi: number): number {
  return psi * pressureRatio;
}

function fahrenheitToCelsius(f: number): number {
  return (f - 32) * (5 / 9);
}

export function formatTelemetryValue(signal: SignalSnapshot, units: TelemetryUnitMode): string {
  if (signal.value == null) {
    if (signal.state === "unsupported") return "unsupported";
    if (signal.state === "error") return "ERR";
    return "--";
  }

  const unit = signal.unit.trim();
  if (unit === "psi") {
    return units === "metric"
      ? `${psiToKpa(signal.value).toFixed(0)} kPa`
      : `${signal.value.toFixed(1)} psi`;
  }
  if (unit === "kPa" || unit === "kPa abs") {
    return units === "metric"
      ? `${signal.value.toFixed(0)} kPa`
      : `${(signal.value / pressureRatio).toFixed(1)} psi`;
  }
  if (unit === "F" || unit === "deg F") {
    return units === "metric"
      ? `${fahrenheitToCelsius(signal.value).toFixed(1)} C`
      : `${signal.value.toFixed(1)} F`;
  }
  if (unit === "g/s") return `${signal.value.toFixed(1)} g/s`;
  if (unit === "%") return `${signal.value.toFixed(1)}%`;
  if (unit === "V") return `${signal.value.toFixed(1)} V`;
  if (unit === "rpm") return `${signal.value.toFixed(0)} rpm`;
  if (unit === "mph") return `${signal.value.toFixed(1)} mph`;
  if (unit === "mm3") return `${formatSigned(signal.value, 1)} mm3`;
  if (unit.length === 0) return signal.value.toFixed(1);
  return `${signal.value.toFixed(1)} ${unit}`;
}

function sectionSlug(category: CapabilitySectionCategory): string {
  return category.toLowerCase().replace(/[^a-z0-9]+/g, "-");
}

function isPairSignal(signal: SignalSnapshot): signal is PairSignal {
  return signal.composition.kind === "pair";
}

function isTableRowSignal(signal: SignalSnapshot): signal is TableRowSignal {
  return signal.composition.kind === "table_row";
}

function categoryIcon(category: CapabilitySectionCategory) {
  switch (category) {
    case "Turbo":
      return <Activity size={14} />;
    case "Fuel":
      return <Fuel size={14} />;
    case "Transmission":
      return <Table2 size={14} />;
    case "Discovery":
      return <ListTree size={14} />;
    default:
      return <Gauge size={14} />;
  }
}

function signalByKey(signals: SignalSnapshot[], keys: string[]): SignalSnapshot | undefined {
  const keySet = new Set(keys);
  return signals.find((signal) => keySet.has(signal.key));
}

function runtimeStateForTile(signal: SignalSnapshot | undefined): StateKind {
  if (!signal) return "muted";
  if (signal.state === "error") return "crit";
  if (signal.state === "waiting" || signal.state === "unsupported") return "muted";
  if (signal.state === "cached") return "warn";
  return "ok";
}

function criticalTiles(snapshot: DiagnosticSnapshot, signals: SignalSnapshot[], unitMode: TelemetryUnitMode): CriticalTile[] {
  const coolant = signalByKey(signals, ["sae.coolant_temp", "coolant_temp", "generic_coolant", "gas_coolant"]);
  const map = signalByKey(signals, ["sae.intake_map", "map_absolute", "generic_map", "gas_map"]);
  const maf = signalByKey(signals, ["sae.maf", "maf", "generic_maf", "gas_maf"]);
  const mil = snapshot.statuses.find((status) => status.label === "MIL");
  const dtcs = snapshot.statuses.find((status) => status.label === "DTCs");

  return [
    { key: "voltage", label: "Voltage", value: `${snapshot.voltage.toFixed(1)} V`, state: "ok" },
    { key: "rpm", label: "Engine RPM", value: snapshot.rpm.toString(), state: "ok" },
    { key: "speed", label: "Speed", value: `${snapshot.speed_mph} mph`, state: "ok" },
    {
      key: "coolant",
      label: "Coolant",
      value: coolant ? formatTelemetryValue(coolant, unitMode) : "--",
      state: runtimeStateForTile(coolant),
    },
    {
      key: "map",
      label: "MAP",
      value: map ? formatTelemetryValue(map, unitMode) : "--",
      state: runtimeStateForTile(map),
    },
    {
      key: "maf",
      label: "MAF",
      value: maf ? formatTelemetryValue(maf, unitMode) : "--",
      state: runtimeStateForTile(maf),
    },
    { key: "mil", label: "MIL", value: mil?.value ?? "--", state: mil?.state ?? "muted" },
    { key: "dtcs", label: "DTCs", value: dtcs?.value ?? snapshot.dtcs.length.toString(), state: dtcs?.state ?? "muted" },
  ];
}

function signalsForSection(section: CapabilitySection, signals: SignalSnapshot[]): SignalSnapshot[] {
  const byKey = new Map(signals.map((signal) => [signal.key, signal]));
  const selected = section.signal_keys
    .map((key) => byKey.get(key))
    .filter((signal): signal is SignalSnapshot => signal != null);

  if (section.category === "Discovery") return selected.filter((signal) => signal.confidence === "Candidate");
  if (
    section.category === "Diagnostics" ||
    section.category === "ActiveTests" ||
    section.category === "Evidence" ||
    section.category === "Replay" ||
    section.category === "Raw" ||
    section.category === "Settings"
  ) {
    return [];
  }
  return selected.filter(isOperationalSignal);
}

function CriticalTileView({ tile }: { tile: CriticalTile }) {
  const tone = stateKindToken(tile.state);
  return (
    <div className={cx("rounded-md border px-3 py-2", tone.borderClassName, tile.state === "ok" ? "bg-zinc-900/60" : tone.surfaceClassName)}>
      <div className="text-[11px] leading-4 text-zinc-400">{tile.label}</div>
      <div className={cx("mt-1 font-mono text-lg font-semibold leading-6 telemetry-value", tile.state === "ok" ? "text-zinc-100" : tone.textClassName)}>
        {tile.value}
      </div>
    </div>
  );
}

function SignalCell({ signal, unitMode }: { signal: SignalSnapshot; unitMode: TelemetryUnitMode }) {
  const runtime = runtimeStateToken(signal.state);
  const trust = trustToken(signal.confidence);

  return (
    <div className={cx("rounded-md border px-3 py-2", runtime.borderClassName, runtime.surfaceClassName)}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className={cx("truncate text-[11px] uppercase leading-4", diagnosticLabelClassName)} title={signal.label}>
            {signal.label}
          </div>
          <div className={cx("mt-1 font-mono text-base font-semibold leading-6", runtime.valueClassName)}>
            {formatTelemetryValue(signal, unitMode)}
          </div>
        </div>
        <div className="flex flex-col items-end gap-1">
          <span className={cx("rounded-sm border px-1.5 py-0.5 text-[10px] font-semibold uppercase", runtime.badgeClassName)}>
            {runtime.label}
          </span>
          <span className={cx("rounded-sm border px-1.5 py-0.5 text-[10px] font-semibold uppercase", trust.badgeClassName)}>
            {trust.label}
          </span>
        </div>
      </div>
      <div className={cx("mt-2 flex flex-wrap gap-2 text-[11px] leading-4", diagnosticSecondaryTextClassName)}>
        <span>{signal.module}</span>
        {signal.provenance.slice(0, 1).map((item) => (
          <span className={trust.textClassName} key={item}>
            {item}
          </span>
        ))}
      </div>
    </div>
  );
}

function PairGroup({ label, signals, unitMode }: { label: string; signals: PairSignal[]; unitMode: TelemetryUnitMode }) {
  const roleOrder: Record<string, number> = { actual: 0, desired: 1, error: 2, delta: 3 };
  const sorted = [...signals].sort((a, b) => (roleOrder[a.composition.role] ?? 99) - (roleOrder[b.composition.role] ?? 99));

  return (
    <div className="rounded-md border border-zinc-800 bg-black/20 p-3">
      <div className="mb-2 text-xs font-semibold uppercase leading-4 text-zinc-400">{label}</div>
      <div className="grid gap-2 md:grid-cols-2 2xl:grid-cols-4">
        {sorted.map((signal) => (
          <SignalCell key={signal.key} signal={signal} unitMode={unitMode} />
        ))}
      </div>
    </div>
  );
}

function TableGroup({ label, signals, unitMode }: { label: string; signals: TableRowSignal[]; unitMode: TelemetryUnitMode }) {
  const sorted = [...signals].sort((a, b) => a.composition.row_index - b.composition.row_index);
  const unit = sorted.find((signal) => signal.unit.length > 0)?.unit ?? "";

  return (
    <div className="rounded-md border border-zinc-800 bg-black/20 p-3">
      <div className="mb-2 flex items-center justify-between gap-3">
        <div className="text-xs font-semibold uppercase leading-4 text-zinc-400">{label}</div>
        <div className="text-[11px] leading-4 text-zinc-400">{unit}</div>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[520px] table-fixed border-collapse text-sm">
          <thead>
            <tr>
              {sorted.map((signal) => (
                <th
                  className="border border-zinc-800 px-2 py-1 text-center text-[11px] font-medium text-zinc-400"
                  key={signal.key}
                >
                  {signal.composition.row_label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            <tr>
              {sorted.map((signal) => {
                const runtime = runtimeStateToken(signal.state);
                return (
                  <td
                    className={cx("border border-zinc-800 px-2 py-2 text-center font-mono font-semibold telemetry-value", runtime.valueClassName)}
                    key={signal.key}
                  >
                    {formatTelemetryValue(signal, unitMode).replace(` ${unit}`, "")}
                  </td>
                );
              })}
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  );
}

function SectionPanel({
  section,
  signals,
  unitMode,
}: {
  section: CapabilitySection;
  signals: SignalSnapshot[];
  unitMode: TelemetryUnitMode;
}) {
  const pairs = new Map<string, PairSignal[]>();
  const tables = new Map<string, TableRowSignal[]>();
  const scalars: SignalSnapshot[] = [];
  const derived: SignalSnapshot[] = [];

  for (const signal of signals) {
    if (isPairSignal(signal)) {
      const group = pairs.get(signal.composition.group_key) ?? [];
      group.push(signal);
      pairs.set(signal.composition.group_key, group);
    } else if (isTableRowSignal(signal)) {
      const group = tables.get(signal.composition.table_key) ?? [];
      group.push(signal);
      tables.set(signal.composition.table_key, group);
    } else if (signal.composition.kind === "derived") {
      derived.push(signal);
    } else {
      scalars.push(signal);
    }
  }

  return (
    <section
      className="rounded-md border border-zinc-700 bg-zinc-900/70"
      data-testid={`telemetry-section-${sectionSlug(section.category)}`}
    >
      <div className="flex h-9 items-center gap-2 border-b border-zinc-800 px-3 text-[11px] font-semibold uppercase text-zinc-400">
        {categoryIcon(section.category)}
        <span>{section.label}</span>
      </div>
      <div className="space-y-3 p-3">
        {scalars.length > 0 ? (
          <div className="grid gap-2 md:grid-cols-2 2xl:grid-cols-3">
            {scalars.map((signal) => (
              <SignalCell key={signal.key} signal={signal} unitMode={unitMode} />
            ))}
          </div>
        ) : null}
        {[...pairs.entries()].map(([groupKey, groupSignals]) => {
          const label = groupSignals.find((signal) => signal.composition.group_label)?.composition.group_label ?? groupKey;
          return <PairGroup key={groupKey} label={label} signals={groupSignals} unitMode={unitMode} />;
        })}
        {[...tables.entries()].map(([tableKey, tableSignals]) => {
          const label = tableSignals.find((signal) => signal.composition.table_label)?.composition.table_label ?? tableKey;
          return <TableGroup key={tableKey} label={label} signals={tableSignals} unitMode={unitMode} />;
        })}
        {derived.length > 0 ? (
          <div className="grid gap-2 md:grid-cols-2 2xl:grid-cols-3">
            {derived.map((signal) => (
              <SignalCell key={signal.key} signal={signal} unitMode={unitMode} />
            ))}
          </div>
        ) : null}
      </div>
    </section>
  );
}

export function TelemetryBoard({ snapshot, unitMode }: TelemetryBoardProps) {
  const signals = snapshot.signals ?? [];
  const sections = (snapshot.capability_sections ?? [])
    .filter((section) => section.visible)
    .map((section) => ({ section, signals: signalsForSection(section, signals) }))
    .filter(({ signals: sectionSignals }) => sectionSignals.length > 0);
  const evidenceSignals = signals
    .filter((signal) => signal.evidence != null || signal.confidence === "Candidate" || signal.state === "cached")
    .slice(0, 4);

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid="telemetry-board">
      <section className="rounded-md border border-zinc-700 bg-zinc-900/70">
        <div className="flex h-9 items-center gap-2 border-b border-zinc-800 px-3 text-[11px] font-semibold uppercase text-zinc-400">
          <Radio size={14} />
          <span>Primary telemetry</span>
        </div>
        <div className="grid gap-2 p-3 sm:grid-cols-2 lg:grid-cols-4">
          {criticalTiles(snapshot, signals, unitMode).map((tile) => (
            <CriticalTileView key={tile.key} tile={tile} />
          ))}
        </div>
      </section>

      <div className="grid min-w-0 gap-3 2xl:grid-cols-[minmax(0,1fr)_380px]">
        <div className="flex min-w-0 flex-col gap-3">
          {sections.map(({ section, signals: sectionSignals }) => (
            <SectionPanel key={section.id ?? section.category} section={section} signals={sectionSignals} unitMode={unitMode} />
          ))}
        </div>

        <aside className="flex min-w-0 flex-col gap-3">
          <section className="rounded-md border border-zinc-700 bg-zinc-900/70">
            <div className="flex h-9 items-center gap-2 border-b border-zinc-800 px-3 text-[11px] font-semibold uppercase text-zinc-400">
              <AlertTriangle size={14} />
              <span>Evidence lane</span>
            </div>
            <div className="space-y-2 p-3">
              {evidenceSignals.length === 0 ? (
                <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-3 text-sm text-zinc-400">
                  No signal evidence attached to this snapshot.
                </div>
              ) : (
                evidenceSignals.map((signal) => (
                  <EvidenceDisclosure key={signal.key} signal={signal} unitMode={unitMode} />
                ))
              )}
            </div>
          </section>
        </aside>
      </div>
    </div>
  );
}
