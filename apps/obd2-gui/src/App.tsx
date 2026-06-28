import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  AlertTriangle,
  Cable,
  CircleDot,
  Database,
  FileText,
  Fuel,
  Gauge,
  ListTree,
  Pause,
  Play,
  Radio,
  RotateCcw,
  Save,
  Settings,
  Square,
  Table2,
  Thermometer,
  Wind,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fallbackSnapshot } from "./mockData";
import type { CylinderBalance, DiagnosticSnapshot, ModuleScan, StateKind } from "./types";

type UnitMode = "us" | "metric";

type RecordingKind = "structured" | "raw" | "compressed" | "unknown";

interface RecordingSummary {
  name: string;
  sizeBytes: number;
  kind: RecordingKind;
  detail: string;
  sessionId?: string;
  started?: string;
  vehicle?: string;
  vin?: string;
  pollMs?: number;
  frameCount?: number;
  eventCount?: number;
  durationMs?: number;
  pidFrames?: number;
  enhancedFrames?: number;
  dtcFrames?: number;
  voltageFrames?: number;
  o2Frames?: number;
  readEvents?: number;
  writeEvents?: number;
  noteEvents?: number;
  preview: string[];
  warning?: string;
}

const stateClasses: Record<StateKind, string> = {
  ok: "text-emerald-300",
  warn: "text-amber-300",
  crit: "text-red-300",
  muted: "text-zinc-500",
};

const panelClass =
  "rounded-md border border-zinc-700/80 bg-zinc-950/58 shadow-[0_18px_60px_rgba(0,0,0,0.25)]";

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function formatSigned(value: number, digits = 1): string {
  const formatted = value.toFixed(digits);
  return value > 0 ? `+${formatted}` : formatted;
}

function psiToKpa(psi: number): number {
  return psi * 6.894757;
}

function fahrenheitToCelsius(f: number): number {
  return (f - 32) * (5 / 9);
}

function pressure(valuePsi: number | null, units: UnitMode): string {
  if (valuePsi == null) return "--";
  if (units === "metric") return `${psiToKpa(valuePsi).toFixed(0)} kPa`;
  return `${valuePsi.toFixed(1)} psi`;
}

function temperature(valueF: number | null, units: UnitMode): string {
  if (valueF == null) return "--";
  if (units === "metric") return `${fahrenheitToCelsius(valueF).toFixed(1)} C`;
  return `${valueF.toFixed(1)} F`;
}

function maf(valueLbMin: number, units: UnitMode): string {
  if (units === "metric") return `${(valueLbMin * 7.559873).toFixed(1)} g/s`;
  return `${valueLbMin.toFixed(1)} lb/min`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDuration(ms: number | undefined): string {
  if (ms == null) return "--";
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function matchesMagic(bytes: Uint8Array, magic: string): boolean {
  if (bytes.length < magic.length) return false;
  for (let i = 0; i < magic.length; i += 1) {
    if (bytes[i] !== magic.charCodeAt(i)) return false;
  }
  return true;
}

async function inspectRecordingFile(file: File): Promise<RecordingSummary> {
  const bytes = new Uint8Array(await file.arrayBuffer());

  if (matchesMagic(bytes, "OBD2REC\u0001")) {
    return parseStructuredRecording(file, bytes, 1);
  }
  if (matchesMagic(bytes, "OBD2REC\u0002")) {
    return parseStructuredRecording(file, bytes, 2);
  }
  if (file.name.endsWith(".obd2rec.gz")) {
    return {
      name: file.name,
      sizeBytes: file.size,
      kind: "compressed",
      detail: "compressed recording",
      preview: [],
      warning: "Compressed .obd2rec.gz inspection needs the Rust reader path. Pick an uncompressed .obd2rec for now.",
    };
  }

  const text = new TextDecoder().decode(bytes);
  if (text.startsWith("# obd2-raw")) {
    return parseRawCapture(file, text);
  }

  return {
    name: file.name,
    sizeBytes: file.size,
    kind: "unknown",
    detail: "unknown file",
    preview: text.split(/\r?\n/).slice(0, 16),
    warning: "Expected .obd2rec, .obd2rec.gz, or .obd2raw.",
  };
}

function parseStructuredRecording(file: File, bytes: Uint8Array, version: 1 | 2): RecordingSummary {
  if (bytes.length < 12) {
    throw new Error("recording is too short to contain a header");
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const headerLen = view.getUint32(8, true);
  const headerStart = 12;
  const headerEnd = headerStart + headerLen;
  if (headerEnd > bytes.length) {
    throw new Error("recording header length exceeds file size");
  }

  const headerText = new TextDecoder().decode(bytes.subarray(headerStart, headerEnd));
  const header = JSON.parse(headerText) as {
    session_id?: string;
    start_time?: string;
    vin?: string | null;
    vehicle_name?: string | null;
    poll_interval_ms?: number;
  };

  let offset = headerEnd;
  let frameCount = 0;
  let durationMs = 0;
  let pidFrames = 0;
  let enhancedFrames = 0;
  let dtcFrames = 0;
  let voltageFrames = 0;
  let o2Frames = 0;
  const preview: string[] = [];

  while (offset + 14 <= bytes.length) {
    const frameOffset = view.getUint32(offset + 1, true);
    const frameType = bytes[offset];
    const pid = bytes[offset + 5];
    durationMs = Math.max(durationMs, frameOffset);
    frameCount += 1;

    if (frameType === 0x01) pidFrames += 1;
    else if (frameType === 0x02) voltageFrames += 1;
    else if (frameType === 0x03) dtcFrames += 1;
    else if (frameType === 0x04) enhancedFrames += 1;
    else if (frameType === 0x05) o2Frames += 1;

    if (preview.length < 12) {
      preview.push(`${frameOffset}ms type=0x${frameType.toString(16).padStart(2, "0")} pid=0x${pid.toString(16).padStart(2, "0")}`);
    }

    offset += 14;
    if (version >= 2) {
      if (offset >= bytes.length) break;
      const rawLen = bytes[offset];
      offset += 1 + rawLen;
    }
  }

  const warning = offset === bytes.length ? undefined : "Trailing bytes were left after frame parsing.";

  return {
    name: file.name,
    sizeBytes: file.size,
    kind: "structured",
    detail: `OBD2REC v${version}`,
    sessionId: header.session_id,
    started: header.start_time,
    vehicle: header.vehicle_name ?? undefined,
    vin: header.vin ?? undefined,
    pollMs: header.poll_interval_ms,
    frameCount,
    durationMs,
    pidFrames,
    enhancedFrames,
    dtcFrames,
    voltageFrames,
    o2Frames,
    preview,
    warning,
  };
}

function parseRawCapture(file: File, text: string): RecordingSummary {
  const lines = text.split(/\r?\n/);
  const headerLines = lines.filter((line) => line.startsWith("#"));
  const dataLines = lines.filter((line) => line.trim() !== "" && !line.startsWith("#"));
  const firstHeader = headerLines[0]?.replace(/^#\s*/, "") ?? "obd2-raw";
  const started = headerLines
    .find((line) => line.startsWith("# started="))
    ?.replace("# started=", "");

  let durationMs = 0;
  let readEvents = 0;
  let writeEvents = 0;
  let noteEvents = 0;

  for (const line of dataLines) {
    const match = line.match(/^(\d+(?:\.\d+)?)\s+([A-Z][.\w]*)\s/);
    if (!match) continue;
    durationMs = Math.max(durationMs, Math.round(Number(match[1]) * 1000));
    if (match[2].startsWith("R")) readEvents += 1;
    else if (match[2].startsWith("W")) writeEvents += 1;
    else if (match[2].startsWith("N")) noteEvents += 1;
  }

  return {
    name: file.name,
    sizeBytes: file.size,
    kind: "raw",
    detail: firstHeader,
    started,
    eventCount: dataLines.length,
    durationMs,
    readEvents,
    writeEvents,
    noteEvents,
    preview: dataLines.slice(0, 14),
    warning: "Raw captures are inspectable here; structured dashboard replay still requires conversion to the .obd2rec frame stream.",
  };
}

function Panel({
  title,
  icon,
  children,
  className = "",
  bodyClassName = "p-3",
  testId,
}: {
  title: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  testId?: string;
}) {
  return (
    <section className={`${panelClass} ${className}`} data-testid={testId}>
      <div className="flex h-9 items-center gap-2 border-b border-zinc-800/90 px-3 text-[11px] font-semibold uppercase text-zinc-400">
        {icon}
        <span>{title}</span>
      </div>
      <div className={bodyClassName}>{children}</div>
    </section>
  );
}

function Toolbar({
  snapshot,
  unitMode,
  setUnitMode,
  refresh,
  lastRefresh,
  recording,
  replayMode,
  onToggleRecording,
  onToggleReplay,
}: {
  snapshot: DiagnosticSnapshot;
  unitMode: UnitMode;
  setUnitMode: (mode: UnitMode) => void;
  refresh: () => void;
  lastRefresh: Date;
  recording: boolean;
  replayMode: boolean;
  onToggleRecording: () => void;
  onToggleReplay: () => void;
}) {
  return (
    <header className="sticky top-0 z-10 border-b border-zinc-800 bg-[#111416]/96 px-4 py-3 backdrop-blur">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex min-w-0 items-center gap-4">
          <div className="flex h-9 w-9 items-center justify-center rounded-md border border-cyan-400/40 bg-cyan-400/10 text-cyan-200">
            <Gauge size={19} />
          </div>
          <div className="min-w-0">
            <div className="truncate text-sm font-semibold text-cyan-100">{snapshot.vehicle}</div>
            <div className="mt-0.5 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-zinc-500">
              <span>VIN {snapshot.vin}</span>
              <span>{snapshot.protocol}</span>
              <span>{snapshot.poll_ms} ms poll</span>
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button className="inline-flex h-8 items-center gap-2 rounded-md border border-emerald-500/40 bg-emerald-500/10 px-3 text-xs font-semibold text-emerald-200">
            <Cable size={14} />
            Connected
          </button>
          <button
            className="inline-flex h-8 items-center rounded-md border border-zinc-700 bg-zinc-900 px-3 text-xs font-semibold text-zinc-200 hover:border-zinc-500"
            onClick={() => setUnitMode(unitMode === "us" ? "metric" : "us")}
          >
            {unitMode === "us" ? "US" : "Metric"}
          </button>
          <button
            aria-pressed={recording}
            className={`inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs font-semibold ${
              recording
                ? "border-red-400/50 bg-red-500/15 text-red-100"
                : "border-zinc-700 bg-zinc-900 text-zinc-200 hover:border-zinc-500"
            }`}
            onClick={onToggleRecording}
          >
            {recording ? <Square size={14} /> : <Save size={14} />}
            {recording ? "Stop Rec" : "Record"}
          </button>
          <button
            aria-pressed={replayMode}
            className={`inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs font-semibold ${
              replayMode
                ? "border-cyan-400/50 bg-cyan-400/15 text-cyan-100"
                : "border-zinc-700 bg-zinc-900 text-zinc-200 hover:border-zinc-500"
            }`}
            onClick={onToggleReplay}
          >
            {replayMode ? <Radio size={14} /> : <Play size={14} />}
            {replayMode ? "Live" : "Replay"}
          </button>
          <button
            className="inline-flex h-8 items-center gap-2 rounded-md border border-zinc-700 bg-zinc-900 px-3 text-xs font-semibold text-zinc-200 hover:border-zinc-500"
            onClick={refresh}
          >
            <RotateCcw size={14} />
            Refresh
          </button>
          <div className="hidden min-w-[112px] text-right text-[11px] text-zinc-500 xl:block">
            {lastRefresh.toLocaleTimeString()}
          </div>
        </div>
      </div>
    </header>
  );
}

function StatusStrip({
  snapshot,
  recording,
  replayMode,
}: {
  snapshot: DiagnosticSnapshot;
  recording: boolean;
  replayMode: boolean;
}) {
  const statuses = snapshot.statuses.map((item) =>
    item.label === "Record"
      ? {
          ...item,
          value: recording ? "ON" : "ready",
          state: recording ? ("crit" as StateKind) : ("warn" as StateKind),
        }
      : item,
  );
  if (replayMode) {
    statuses.push({
      label: "Replay",
      value: "ON",
      state: "warn",
    });
  }

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 md:grid-cols-4 xl:grid-cols-8">
      <div className="rounded-md border border-zinc-800 bg-zinc-950/70 px-3 py-2">
        <div className="text-[11px] text-zinc-500">Voltage</div>
        <div className="mt-1 text-lg font-semibold text-yellow-300">{snapshot.voltage.toFixed(1)} V</div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-950/70 px-3 py-2">
        <div className="text-[11px] text-zinc-500">Engine RPM</div>
        <div className="mt-1 text-lg font-semibold text-emerald-300">{snapshot.rpm}</div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-950/70 px-3 py-2">
        <div className="text-[11px] text-zinc-500">Speed</div>
        <div className="mt-1 text-lg font-semibold text-zinc-100">{snapshot.speed_mph} mph</div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-950/70 px-3 py-2">
        <div className="text-[11px] text-zinc-500">Source</div>
        <div className="mt-1 text-lg font-semibold text-cyan-200">{snapshot.connection}</div>
      </div>
      {statuses.map((item) => (
        <div className="rounded-md border border-zinc-800 bg-zinc-950/70 px-3 py-2" key={item.label}>
          <div className="text-[11px] text-zinc-500">{item.label}</div>
          <div className={`mt-1 text-lg font-semibold ${stateClasses[item.state]}`}>{item.value}</div>
        </div>
      ))}
    </div>
  );
}

function VgtPanel({ snapshot }: { snapshot: DiagnosticSnapshot }) {
  const errorState =
    Math.abs(snapshot.vgt.error_pct) <= 3
      ? "text-emerald-300"
      : Math.abs(snapshot.vgt.error_pct) <= 5
        ? "text-amber-300"
        : "text-red-300";

  return (
    <Panel title="Enhanced PIDs" icon={<Activity size={14} />} className="min-h-[430px]">
      <div className="grid min-h-[340px] content-start gap-3">
        <div className="rounded-md border border-zinc-800 bg-black/20 p-3">
          <div className="text-[11px] uppercase text-zinc-500">VGT vane position</div>
          <div className="mt-3 grid grid-cols-3 gap-2 text-center">
            <GaugeMetric label="Actual" value={`${snapshot.vgt.actual_pct.toFixed(1)}%`} tone="cyan" />
            <GaugeMetric label="Desired" value={`${snapshot.vgt.desired_pct.toFixed(1)}%`} tone="emerald" />
            <div>
              <div className="text-[11px] text-zinc-500">Error</div>
              <div className={`mt-1 text-2xl font-semibold ${errorState}`}>
                {formatSigned(snapshot.vgt.error_pct, 1)}%
              </div>
            </div>
          </div>
        </div>
        <CylinderTable cylinders={snapshot.cylinders} />
      </div>
    </Panel>
  );
}

function GaugeMetric({ label, value, tone }: { label: string; value: string; tone: "cyan" | "emerald" }) {
  const color = tone === "cyan" ? "text-cyan-200" : "text-emerald-300";
  return (
    <div>
      <div className="text-[11px] text-zinc-500">{label}</div>
      <div className={`mt-1 text-2xl font-semibold ${color}`}>{value}</div>
    </div>
  );
}

function CylinderTable({ cylinders }: { cylinders: CylinderBalance[] }) {
  return (
    <div className="rounded-md border border-zinc-800 bg-black/20 p-3">
      <div className="mb-2 flex items-center justify-between">
        <div className="text-[11px] uppercase text-zinc-500">Injector balance</div>
        <div className="text-[11px] text-zinc-500">mm3</div>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[520px] table-fixed border-collapse text-sm">
          <thead>
            <tr>
              <th className="border border-zinc-800 px-2 py-1 text-left text-[11px] font-medium text-zinc-500">
                Cyl
              </th>
              {cylinders.map((item) => (
                <th
                  className="border border-zinc-800 px-2 py-1 text-center text-[11px] font-medium text-zinc-500"
                  key={item.cylinder}
                >
                  {item.cylinder}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            <tr>
              <td className="border border-zinc-800 px-2 py-2 text-xs text-zinc-500">mm3</td>
              {cylinders.map((item) => {
                const tone =
                  Math.abs(item.mm3) >= 4
                    ? "text-red-300"
                    : Math.abs(item.mm3) >= 2
                      ? "text-amber-300"
                      : "text-cyan-200";
                return (
                  <td className={`border border-zinc-800 px-2 py-2 text-center font-semibold ${tone}`} key={item.cylinder}>
                    {formatSigned(item.mm3)}
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

function FuelRailPanel({ snapshot, unitMode }: { snapshot: DiagnosticSnapshot; unitMode: UnitMode }) {
  return (
    <Panel title="Fuel rail" icon={<Fuel size={14} />}>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <Readout label="Actual" value={pressure(snapshot.fuel_rail.actual_psi, unitMode)} />
        <Readout label="Desired" value={pressure(snapshot.fuel_rail.desired_psi, unitMode)} muted={snapshot.fuel_rail.desired_psi == null} />
        <Readout label="Delta" value={pressure(snapshot.fuel_rail.delta_psi, unitMode)} muted={snapshot.fuel_rail.delta_psi == null} />
      </div>
      <div className="mt-3 rounded-md border border-amber-400/30 bg-amber-400/5 px-3 py-2 text-xs text-amber-200">
        Desired pressure is displayed only after the DID is verified on this ECM.
      </div>
    </Panel>
  );
}

function ModuleScanPanel({ modules }: { modules: ModuleScan[] }) {
  return (
    <Panel title="Module scan" icon={<ListTree size={14} />}>
      <table className="w-full border-collapse text-sm">
        <thead>
          <tr className="text-left text-[11px] uppercase text-zinc-500">
            <th className="border-b border-zinc-800 pb-2 font-medium">Module</th>
            <th className="border-b border-zinc-800 pb-2 font-medium">03</th>
            <th className="border-b border-zinc-800 pb-2 font-medium">07</th>
            <th className="border-b border-zinc-800 pb-2 font-medium">0A</th>
          </tr>
        </thead>
        <tbody>
          {modules.map((module) => (
            <tr className="border-b border-zinc-900 last:border-0" key={module.module}>
              <td className="py-2 font-semibold text-zinc-200">{module.module}</td>
              <td className={scanClass(module.stored)}>{module.stored}</td>
              <td className={scanClass(module.pending)}>{module.pending}</td>
              <td className={scanClass(module.permanent)}>{module.permanent}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

function scanClass(value: string): string {
  if (value === "empty") return "py-2 text-emerald-300";
  if (value === "probe") return "py-2 text-cyan-200";
  if (value === "no data") return "py-2 text-zinc-500";
  return "py-2 text-yellow-300";
}

function AlertsPanel({ alerts }: { alerts: string[] }) {
  return (
    <Panel title="Alerts" icon={<AlertTriangle size={14} />} className="min-h-[340px]">
      <div className="min-h-[260px] space-y-2 overflow-y-auto pr-1">
        {alerts.length === 0 ? (
          <div className="text-sm text-zinc-500">No active alerts</div>
        ) : (
          alerts.map((alert) => (
            <div className="rounded-md border border-amber-500/25 bg-amber-500/7 px-3 py-2 text-sm text-amber-100" key={alert}>
              {alert}
            </div>
          ))
        )}
      </div>
    </Panel>
  );
}

function Readout({
  label,
  value,
  muted = false,
}: {
  label: string;
  value: string;
  muted?: boolean;
}) {
  return (
    <div className="rounded-md border border-zinc-800 bg-black/20 px-3 py-3">
      <div className="text-[11px] uppercase text-zinc-500">{label}</div>
      <div className={`mt-2 text-xl font-semibold ${muted ? "text-zinc-500" : "text-emerald-300"}`}>{value}</div>
    </div>
  );
}

function LiveReadouts({ snapshot, unitMode }: { snapshot: DiagnosticSnapshot; unitMode: UnitMode }) {
  return (
    <div className="grid gap-3 md:grid-cols-3">
      <Panel title="Intake MAP" icon={<Gauge size={14} />}>
        <Readout label="Manifold absolute" value={pressure(snapshot.map_psi, unitMode)} />
      </Panel>
      <Panel title="Boost" icon={<Zap size={14} />}>
        <Readout label="Derived boost" value={pressure(snapshot.boost_psi, unitMode)} />
      </Panel>
      <Panel title="MAF" icon={<Wind size={14} />}>
        <Readout label="Mass air flow" value={maf(snapshot.maf_lb_min, unitMode)} />
      </Panel>
    </div>
  );
}

function TemperaturePanel({ snapshot, unitMode }: { snapshot: DiagnosticSnapshot; unitMode: UnitMode }) {
  const rows = [
    ["Coolant", snapshot.temperatures.coolant_f],
    ["Intake Air", snapshot.temperatures.intake_air_f],
    ["Oil", snapshot.temperatures.oil_f],
    ["Trans", snapshot.temperatures.trans_f],
    ["Ambient", snapshot.temperatures.ambient_f],
  ] as const;

  return (
    <Panel title="Temperatures" icon={<Thermometer size={14} />}>
      <div className="grid grid-cols-1 gap-x-6 gap-y-3 sm:grid-cols-2">
        {rows.map(([label, value]) => (
          <div className="flex items-center justify-between border-b border-zinc-900 pb-2" key={label}>
            <span className="text-sm text-zinc-500">{label}</span>
            <span className={value == null ? "text-sm font-semibold text-zinc-500" : "text-sm font-semibold text-emerald-300"}>
              {temperature(value, unitMode)}
            </span>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function DtcPanel() {
  return (
    <Panel title="DTCs" icon={<FileText size={14} />}>
      <div className="rounded-md border border-emerald-500/30 bg-emerald-500/6 px-3 py-3 text-sm text-emerald-200">
        No diagnostic codes
      </div>
      <div className="mt-3 grid grid-cols-1 gap-2 text-xs text-zinc-500 sm:grid-cols-3">
        <div className="rounded-md border border-zinc-800 px-3 py-2">Stored 0</div>
        <div className="rounded-md border border-zinc-800 px-3 py-2">Pending 0</div>
        <div className="rounded-md border border-zinc-800 px-3 py-2">Permanent --</div>
      </div>
    </Panel>
  );
}

function ReadinessPanel() {
  return (
    <Panel title="Readiness" icon={<CircleDot size={14} />}>
      <div className="flex items-center justify-between">
        <span className="text-sm text-zinc-500">MIL</span>
        <span className="text-sm font-semibold text-emerald-300">OFF</span>
      </div>
      <div className="mt-3 flex items-center justify-between">
        <span className="text-sm text-zinc-500">Monitor data</span>
        <span className="text-sm font-semibold text-zinc-500">waiting</span>
      </div>
      <div className="mt-4 h-2 rounded-full bg-zinc-900">
        <div className="h-2 w-1/5 rounded-full bg-cyan-300" />
      </div>
    </Panel>
  );
}

function RawPanel({ snapshot }: { snapshot: DiagnosticSnapshot }) {
  const payload = useMemo(() => JSON.stringify(snapshot, null, 2), [snapshot]);
  return (
    <Panel
      title="Raw snapshot"
      icon={<Database size={14} />}
      className="flex h-[calc(100vh-238px)] min-h-[420px] flex-col"
      bodyClassName="flex min-h-0 flex-1 p-3"
      testId="raw-snapshot-panel"
    >
      <pre className="min-h-0 flex-1 overflow-auto rounded-md bg-black/30 p-3 text-xs leading-5 text-zinc-400">
        {payload}
      </pre>
    </Panel>
  );
}

function ReplayPanel({
  snapshot,
  selectedRecording,
  openRecording,
  replayError,
  replayRunning,
  replayPaused,
  setReplayPaused,
  setReplayRunning,
  exitReplay,
}: {
  snapshot: DiagnosticSnapshot;
  selectedRecording: RecordingSummary | null;
  openRecording: (file: File) => void;
  replayError: string | null;
  replayRunning: boolean;
  replayPaused: boolean;
  setReplayPaused: (paused: boolean) => void;
  setReplayRunning: (running: boolean) => void;
  exitReplay: () => void;
}) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  const replayState = selectedRecording == null ? "No file" : replayRunning ? (replayPaused ? "Paused" : "Playing") : "Loaded";
  const progressPct = replayRunning ? 42 : 0;

  return (
    <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_360px]">
      <Panel title="Replay controls" icon={<Play size={14} />} className="min-h-[420px]">
        <div className="rounded-md border border-zinc-800 bg-black/20 p-3">
          <div className="grid gap-3 md:grid-cols-3">
            <Readout label="Session" value={selectedRecording?.sessionId ?? selectedRecording?.name ?? "--"} muted={selectedRecording == null} />
            <Readout label="State" value={replayState} muted={selectedRecording == null} />
            <Readout label="Progress" value={`${progressPct}%`} muted={!replayRunning} />
          </div>
          <div className="mt-4 h-2 rounded-full bg-zinc-900">
            <div className="h-2 rounded-full bg-cyan-300" style={{ width: `${progressPct}%` }} />
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            <input
              ref={inputRef}
              aria-label="Open recording file"
              className="hidden"
              type="file"
              accept=".obd2rec,.obd2rec.gz,.obd2raw"
              onChange={(event) => {
                const file = event.currentTarget.files?.[0];
                if (file) openRecording(file);
                event.currentTarget.value = "";
              }}
            />
            <button
              className="inline-flex h-9 items-center gap-2 rounded-md border border-zinc-700 bg-zinc-900 px-4 text-sm font-semibold text-zinc-200 hover:border-zinc-500"
              onClick={() => inputRef.current?.click()}
            >
              <FileText size={15} />
              Open recording
            </button>
            <button
              className="inline-flex h-9 items-center gap-2 rounded-md border border-emerald-400/50 bg-emerald-400/15 px-4 text-sm font-semibold text-emerald-100 disabled:cursor-not-allowed disabled:border-zinc-800 disabled:bg-zinc-900 disabled:text-zinc-600"
              disabled={selectedRecording == null}
              onClick={() => {
                setReplayRunning(true);
                setReplayPaused(false);
              }}
            >
              {replayRunning ? <RotateCcw size={15} /> : <Play size={15} />}
              {replayRunning ? "Restart" : "Play loaded"}
            </button>
            <button
              className="inline-flex h-9 items-center gap-2 rounded-md border border-cyan-400/50 bg-cyan-400/15 px-4 text-sm font-semibold text-cyan-100 disabled:cursor-not-allowed disabled:border-zinc-800 disabled:bg-zinc-900 disabled:text-zinc-600"
              disabled={selectedRecording == null || !replayRunning}
              onClick={() => setReplayPaused(!replayPaused)}
            >
              {replayPaused ? <Play size={15} /> : <Pause size={15} />}
              {replayPaused ? "Resume" : "Pause"}
            </button>
            <button
              className="inline-flex h-9 items-center gap-2 rounded-md border border-zinc-700 bg-zinc-900 px-4 text-sm font-semibold text-zinc-200 hover:border-zinc-500"
              onClick={exitReplay}
            >
              <Radio size={15} />
              Exit replay
            </button>
          </div>
          {replayError ? (
            <div className="mt-4 rounded-md border border-red-400/30 bg-red-400/7 px-3 py-2 text-xs leading-5 text-red-200">
              {replayError}
            </div>
          ) : null}
          {selectedRecording?.warning ? (
            <div className="mt-4 rounded-md border border-amber-400/30 bg-amber-400/5 px-3 py-2 text-xs leading-5 text-amber-200">
              {selectedRecording.warning}
            </div>
          ) : null}
          {selectedRecording?.preview.length ? (
            <pre className="mt-4 max-h-44 overflow-auto rounded-md border border-zinc-800 bg-black/30 p-3 text-xs leading-5 text-zinc-400">
              {selectedRecording.preview.join("\n")}
            </pre>
          ) : null}
        </div>
      </Panel>
      <Panel title="Replay source" icon={<Database size={14} />} className="min-h-[420px]">
        <div className="space-y-3 text-sm">
          <SettingRow label="Vehicle" value={selectedRecording?.vehicle ?? snapshot.vehicle} />
          <SettingRow label="VIN" value={selectedRecording?.vin ?? snapshot.vin} />
          <SettingRow label="File" value={selectedRecording?.name ?? "none selected"} tone={selectedRecording ? "ok" : "warn"} />
          <SettingRow label="Format" value={selectedRecording?.detail ?? "--"} />
          <SettingRow label="Size" value={selectedRecording ? formatBytes(selectedRecording.sizeBytes) : "--"} />
          <SettingRow label="Duration" value={formatDuration(selectedRecording?.durationMs)} />
          {selectedRecording?.pollMs != null ? <SettingRow label="Poll interval" value={`${selectedRecording.pollMs} ms`} /> : null}
          {selectedRecording?.frameCount != null ? <SettingRow label="Frames" value={selectedRecording.frameCount.toString()} /> : null}
          {selectedRecording?.eventCount != null ? <SettingRow label="Raw events" value={selectedRecording.eventCount.toString()} /> : null}
          {selectedRecording?.pidFrames != null ? <SettingRow label="PID frames" value={selectedRecording.pidFrames.toString()} /> : null}
          {selectedRecording?.enhancedFrames != null ? <SettingRow label="Enhanced frames" value={selectedRecording.enhancedFrames.toString()} /> : null}
          {selectedRecording?.dtcFrames != null ? <SettingRow label="DTC frames" value={selectedRecording.dtcFrames.toString()} /> : null}
          {selectedRecording?.readEvents != null ? <SettingRow label="Read events" value={selectedRecording.readEvents.toString()} /> : null}
          {selectedRecording?.writeEvents != null ? <SettingRow label="Write events" value={selectedRecording.writeEvents.toString()} /> : null}
        </div>
        <div className="mt-4 rounded-md border border-amber-400/30 bg-amber-400/5 px-3 py-2 text-xs leading-5 text-amber-200">
          Opening old files is wired. Playback is still GUI-local until the Rust replay controller is attached to this shell.
        </div>
      </Panel>
    </div>
  );
}

function SettingsPanel({
  snapshot,
  unitMode,
  setUnitMode,
}: {
  snapshot: DiagnosticSnapshot;
  unitMode: UnitMode;
  setUnitMode: (mode: UnitMode) => void;
}) {
  return (
    <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_360px]">
      <Panel title="Runtime settings" icon={<Settings size={14} />} className="min-h-[420px]">
        <div className="grid gap-4 lg:grid-cols-2">
          <section className="rounded-md border border-zinc-800 bg-black/20 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-500">Display units</div>
            <div className="mt-3 flex gap-2">
              <button
                aria-pressed={unitMode === "us"}
                className={`h-9 rounded-md border px-4 text-sm font-semibold ${
                  unitMode === "us"
                    ? "border-cyan-400/50 bg-cyan-400/15 text-cyan-100"
                    : "border-zinc-800 text-zinc-400 hover:text-zinc-200"
                }`}
                onClick={() => setUnitMode("us")}
              >
                US
              </button>
              <button
                aria-pressed={unitMode === "metric"}
                className={`h-9 rounded-md border px-4 text-sm font-semibold ${
                  unitMode === "metric"
                    ? "border-cyan-400/50 bg-cyan-400/15 text-cyan-100"
                    : "border-zinc-800 text-zinc-400 hover:text-zinc-200"
                }`}
                onClick={() => setUnitMode("metric")}
              >
                Metric
              </button>
            </div>
            <div className="mt-3 text-xs leading-5 text-zinc-500">
              Pressure, airflow, and temperature readouts follow this setting.
            </div>
          </section>

          <section className="rounded-md border border-zinc-800 bg-black/20 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-500">Polling</div>
            <div className="mt-3 space-y-3 text-sm">
              <SettingRow label="Standard PID poll" value={`${snapshot.poll_ms} ms`} />
              <SettingRow label="Enhanced refresh" value="2.5 s mock" />
              <SettingRow label="Mode" value="mock snapshot" />
            </div>
          </section>

          <section className="rounded-md border border-zinc-800 bg-black/20 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-500">Adapter</div>
            <div className="mt-3 space-y-3 text-sm">
              <SettingRow label="Protocol" value={snapshot.protocol} />
              <SettingRow label="Transport" value="Tauri command boundary" />
              <SettingRow label="Live serial" value="not attached" tone="warn" />
            </div>
          </section>

          <section className="rounded-md border border-zinc-800 bg-black/20 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-500">Diagnostics</div>
            <div className="mt-3 space-y-3 text-sm">
              <SettingRow label="Enhanced DTC service" value="probe first" tone="warn" />
              <SettingRow label="Desired fuel rail" value="DID unverified" tone="warn" />
              <SettingRow label="Status byte map" value="GM Class 2 pending" />
            </div>
          </section>
        </div>
      </Panel>

      <Panel title="Runtime state" icon={<Radio size={14} />} className="min-h-[420px]">
        <div className="space-y-3 text-sm">
          <SettingRow label="Vehicle" value={snapshot.vehicle} />
          <SettingRow label="VIN" value={snapshot.vin} />
          <SettingRow label="Voltage" value={`${snapshot.voltage.toFixed(1)} V`} />
          <SettingRow label="Connection" value={snapshot.connection} tone="ok" />
        </div>
        <div className="mt-4 rounded-md border border-amber-400/30 bg-amber-400/5 px-3 py-2 text-xs leading-5 text-amber-200">
          These settings are local GUI state right now. Live serial ownership still belongs at the Rust session boundary.
        </div>
      </Panel>
    </div>
  );
}

function SettingRow({
  label,
  value,
  tone = "default",
}: {
  label: string;
  value: string;
  tone?: "default" | "ok" | "warn";
}) {
  const valueClass =
    tone === "ok" ? "text-emerald-300" : tone === "warn" ? "text-amber-300" : "text-zinc-200";
  return (
    <div className="flex items-center justify-between gap-3 border-b border-zinc-900 pb-2 last:border-0 last:pb-0">
      <span className="text-zinc-500">{label}</span>
      <span className={`text-right font-semibold ${valueClass}`}>{value}</span>
    </div>
  );
}

function App() {
  const [snapshot, setSnapshot] = useState<DiagnosticSnapshot>(fallbackSnapshot);
  const [unitMode, setUnitMode] = useState<UnitMode>("us");
  const [lastRefresh, setLastRefresh] = useState(new Date());
  const [view, setView] = useState<"dashboard" | "raw" | "settings" | "replay">("dashboard");
  const [recording, setRecording] = useState(false);
  const [replayMode, setReplayMode] = useState(false);
  const [replayPaused, setReplayPaused] = useState(false);
  const [replayRunning, setReplayRunning] = useState(false);
  const [selectedRecording, setSelectedRecording] = useState<RecordingSummary | null>(null);
  const [replayError, setReplayError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (isTauriRuntime()) {
      try {
        const next = await invoke<DiagnosticSnapshot>("diagnostic_snapshot");
        setSnapshot(next);
      } catch (error) {
        console.error("diagnostic_snapshot failed", error);
      }
    }
    setLastRefresh(new Date());
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => {
      void refresh();
    }, 2_500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const toggleRecording = useCallback(() => {
    setRecording((current) => !current);
  }, []);

  const toggleReplay = useCallback(() => {
    setReplayMode((current) => {
      const next = !current;
      setReplayPaused(false);
      if (!next) setReplayRunning(false);
      setView(next ? "replay" : "dashboard");
      return next;
    });
  }, []);

  const openRecording = useCallback((file: File) => {
    setReplayError(null);
    setReplayRunning(false);
    setReplayPaused(false);
    inspectRecordingFile(file)
      .then((summary) => {
        setSelectedRecording(summary);
        setReplayMode(true);
        setView("replay");
      })
      .catch((error: unknown) => {
        setSelectedRecording(null);
        setReplayError(error instanceof Error ? error.message : String(error));
      });
  }, []);

  const exitReplay = useCallback(() => {
    setReplayMode(false);
    setReplayPaused(false);
    setReplayRunning(false);
    setView("dashboard");
  }, []);

  return (
    <div className="min-h-screen bg-[#090b0d] text-zinc-100">
      <Toolbar
        snapshot={snapshot}
        unitMode={unitMode}
        setUnitMode={setUnitMode}
        refresh={refresh}
        lastRefresh={lastRefresh}
        recording={recording}
        replayMode={replayMode}
        onToggleRecording={toggleRecording}
        onToggleReplay={toggleReplay}
      />
      <main className="mx-auto flex max-w-[1680px] flex-col gap-3 px-4 py-4">
        <StatusStrip snapshot={snapshot} recording={recording} replayMode={replayMode} />
        <div className="flex flex-col gap-2 border-y border-zinc-900 py-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 items-center gap-2 text-xs text-zinc-500">
            <Radio size={14} />
            <span className="truncate">Tauri shell active. Live serial integration will attach at the Rust command boundary.</span>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              className={`h-8 rounded-md px-3 text-xs font-semibold ${view === "dashboard" ? "bg-cyan-400/15 text-cyan-100" : "text-zinc-500 hover:text-zinc-300"}`}
              onClick={() => setView("dashboard")}
            >
              Dashboard
            </button>
            <button
              className={`h-8 rounded-md px-3 text-xs font-semibold ${view === "raw" ? "bg-cyan-400/15 text-cyan-100" : "text-zinc-500 hover:text-zinc-300"}`}
              onClick={() => setView("raw")}
            >
              Raw
            </button>
            <button
              className={`inline-flex h-8 items-center gap-2 rounded-md border border-zinc-800 px-3 text-xs font-semibold ${
                view === "settings" ? "bg-cyan-400/15 text-cyan-100" : "text-zinc-400 hover:text-zinc-200"
              }`}
              onClick={() => setView("settings")}
            >
              <Settings size={14} />
              Settings
            </button>
          </div>
        </div>

        {view === "raw" ? (
          <RawPanel snapshot={snapshot} />
        ) : view === "settings" ? (
          <SettingsPanel snapshot={snapshot} unitMode={unitMode} setUnitMode={setUnitMode} />
        ) : view === "replay" ? (
          <ReplayPanel
            snapshot={snapshot}
            selectedRecording={selectedRecording}
            openRecording={openRecording}
            replayError={replayError}
            replayRunning={replayRunning}
            replayPaused={replayPaused}
            setReplayPaused={setReplayPaused}
            setReplayRunning={setReplayRunning}
            exitReplay={exitReplay}
          />
        ) : (
          <div className="grid gap-3 xl:grid-cols-[360px_minmax(0,1fr)_360px]">
            <div className="flex flex-col gap-3">
              <DtcPanel />
              <ModuleScanPanel modules={snapshot.modules} />
              <ReadinessPanel />
            </div>
            <div className="flex min-w-0 flex-col gap-3">
              <VgtPanel snapshot={snapshot} />
              <FuelRailPanel snapshot={snapshot} unitMode={unitMode} />
              <LiveReadouts snapshot={snapshot} unitMode={unitMode} />
              <TemperaturePanel snapshot={snapshot} unitMode={unitMode} />
            </div>
            <div className="flex flex-col gap-3">
              <AlertsPanel alerts={snapshot.alerts} />
              <Panel title="Protocol" icon={<Table2 size={14} />}>
                <div className="space-y-3 text-sm">
                  <div className="flex items-center justify-between border-b border-zinc-900 pb-2">
                    <span className="text-zinc-500">Bus</span>
                    <span className="font-semibold text-cyan-200">{snapshot.protocol}</span>
                  </div>
                  <div className="flex items-center justify-between border-b border-zinc-900 pb-2">
                    <span className="text-zinc-500">Header</span>
                    <span className="font-semibold text-zinc-200">68 6A F1</span>
                  </div>
                  <div className="flex items-center justify-between border-b border-zinc-900 pb-2">
                    <span className="text-zinc-500">Enhanced DTC</span>
                    <span className="font-semibold text-amber-300">probe first</span>
                  </div>
                </div>
              </Panel>
            </div>
          </div>
        )}
      </main>
    </div>
  );
}

export default App;
