import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  AlertTriangle,
  Cable,
  ChevronDown,
  Database,
  FileText,
  Fuel,
  Gauge,
  ListTree,
  LockKeyhole,
  Pause,
  Play,
  Radio,
  RotateCcw,
  Settings,
  ShieldAlert,
  SlidersHorizontal,
  Square,
  Table2,
  Wind,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TelemetryBoard } from "./components/TelemetryBoard";
import { fallbackSnapshot, gasNoTurboSnapshot, genericObdSnapshot, transmissionSnapshot } from "./mockData";
import type {
  CapabilitySection,
  CapabilitySectionCategory,
  DiagnosticSnapshot,
  DtcSnapshot,
  ModuleScan,
  SignalEvidence,
  SignalSnapshot as CapabilitySignalSnapshot,
  StateKind,
} from "./types";

type UnitMode = "us" | "metric";
type SessionMode = "live" | "recording" | "replay";
type UtilityTabId = "overview" | "active" | "diagnostics" | "raw" | "settings";
type CapabilityTabId = `cap:${string}`;
type TabId = UtilityTabId | CapabilityTabId;

type RecordingKind = "structured" | "raw" | "compressed" | "unknown";
type RunnerCommandReply = "accepted" | "busy" | "not_ready" | "not_running" | "closed";

interface DiagnosticServiceSnapshot {
  key: string;
  label: string;
  module: string;
  state: string;
  detail: string;
}

type CapabilitySnapshot = DiagnosticSnapshot & {
  diagnostic_services?: DiagnosticServiceSnapshot[];
};

interface CategoryTab {
  id: TabId;
  label: string;
  summary: string;
  icon: React.ReactNode;
}

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
  evidencePreview?: string[];
  preview: string[];
  warning?: string;
}

const stateClasses: Record<StateKind, string> = {
  ok: "text-emerald-300",
  warn: "text-amber-300",
  crit: "text-red-400",
  muted: "text-zinc-400",
};

const panelClass =
  "rounded-md border border-zinc-700 bg-zinc-900/60 ring-1 ring-white/5";

function capabilitySnapshot(snapshot: DiagnosticSnapshot): CapabilitySnapshot {
  return snapshot as CapabilitySnapshot;
}

function capabilitySignals(snapshot: DiagnosticSnapshot): CapabilitySignalSnapshot[] {
  return capabilitySnapshot(snapshot).signals ?? [];
}

function capabilitySections(snapshot: DiagnosticSnapshot): CapabilitySection[] {
  return capabilitySnapshot(snapshot).capability_sections?.filter((section) => section.visible) ?? [];
}

function sectionKey(category: string): string {
  return category
    .replace(/([a-z])([A-Z])/g, "$1-$2")
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase();
}

function capabilityTabId(category: CapabilitySectionCategory): CapabilityTabId {
  return `cap:${sectionKey(category)}` as CapabilityTabId;
}

function isCapabilityTab(tab: TabId): tab is CapabilityTabId {
  return tab.startsWith("cap:");
}

function signalMap(signals: CapabilitySignalSnapshot[]): Map<string, CapabilitySignalSnapshot> {
  return new Map(signals.map((signal) => [signal.key, signal]));
}

function isOperationalSignal(signal: CapabilitySignalSnapshot): boolean {
  return signal.confidence !== "Candidate" && signal.confidence !== "Rejected" && signal.failure_policy !== "DoNotPoll";
}

function signalsForSection(section: CapabilitySection, signals: CapabilitySignalSnapshot[]): CapabilitySignalSnapshot[] {
  const byKey = signalMap(signals);
  const selected = section.signal_keys
    .map((key) => byKey.get(key))
    .filter((signal): signal is CapabilitySignalSnapshot => signal != null);

  if (section.category === "Discovery") {
    return selected.filter((signal) => signal.confidence === "Candidate");
  }
  if (section.category === "Evidence") {
    return selected;
  }
  if (section.category === "Diagnostics" || section.category === "ActiveTests") {
    return selected;
  }
  return selected.filter(isOperationalSignal);
}

function capabilitySectionForTab(snapshot: DiagnosticSnapshot, tab: TabId): CapabilitySection | undefined {
  if (!isCapabilityTab(tab)) return undefined;
  const suffix = tab.replace(/^cap:/, "");
  return capabilitySections(snapshot).find((section) => sectionKey(section.category) === suffix);
}

function capabilitySectionIcon(category: CapabilitySectionCategory): React.ReactNode {
  switch (category) {
    case "Powertrain":
      return <Gauge size={14} />;
    case "Turbo":
      return <Activity size={14} />;
    case "Fuel":
      return <Fuel size={14} />;
    case "Transmission":
      return <Table2 size={14} />;
    case "Body":
      return <Cable size={14} />;
    case "Chassis":
      return <ShieldAlert size={14} />;
    case "Emissions":
      return <Wind size={14} />;
    case "Discovery":
      return <ListTree size={14} />;
    case "Diagnostics":
      return <FileText size={14} />;
    case "ActiveTests":
      return <LockKeyhole size={14} />;
    case "Evidence":
      return <Database size={14} />;
    case "Replay":
      return <Play size={14} />;
    case "Raw":
      return <Database size={14} />;
    case "Settings":
      return <Settings size={14} />;
    default:
      return <Gauge size={14} />;
  }
}

function runtimeTone(signal: CapabilitySignalSnapshot): "ok" | "warn" | "crit" | "muted" {
  if (signal.confidence === "Rejected" || signal.failure_policy === "DoNotPoll") return "muted";
  if (signal.state === "ok") return "ok";
  if (signal.state === "cached" || signal.state === "waiting" || signal.confidence === "Candidate") return "warn";
  if (signal.state === "error") return "crit";
  return "muted";
}

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function initialSnapshot(): DiagnosticSnapshot {
  if (!isTauriRuntime()) {
    const fixture = new URLSearchParams(window.location.search).get("fixture");
    if (fixture === "generic-obd") return genericObdSnapshot;
    if (fixture === "gas-no-turbo") return gasNoTurboSnapshot;
    if (fixture === "transmission") return transmissionSnapshot;
    return fallbackSnapshot;
  }
  return {
    ...fallbackSnapshot,
    connection: "connecting live",
    voltage: 0,
    rpm: 0,
    speed_mph: 0,
    alerts: ["Opening live serial session"],
    dtcs: [],
    modules: [],
  };
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

function pressureFromKpa(valueKpa: number | null, units: UnitMode): string {
  if (valueKpa == null) return "--";
  if (units === "metric") return `${valueKpa.toFixed(0)} kPa`;
  return `${(valueKpa / 6.894757).toFixed(1)} psi`;
}

function temperature(valueF: number | null, units: UnitMode): string {
  if (valueF == null) return "--";
  if (units === "metric") return `${fahrenheitToCelsius(valueF).toFixed(1)} C`;
  return `${valueF.toFixed(1)} F`;
}

function maf(valueGramsPerSec: number): string {
  return `${valueGramsPerSec.toFixed(1)} g/s`;
}

function signalDisplayValue(signal: CapabilitySignalSnapshot, units: UnitMode): string {
  if (signal.value == null) {
    if (signal.state === "unsupported") return "unsupported";
    if (signal.state === "error") return "ERR";
    return "--";
  }

  const unit = signal.unit.trim();
  if (unit === "psi") return pressure(signal.value, units);
  if (unit === "kPa" || unit === "kPa abs") return pressureFromKpa(signal.value, units);
  if (unit === "F") return temperature(signal.value, units);
  if (unit === "g/s") return maf(signal.value);
  if (unit === "%") return `${signal.value.toFixed(1)}%`;
  if (unit === "V") return `${signal.value.toFixed(1)} V`;
  if (unit === "rpm") return `${signal.value.toFixed(0)} rpm`;
  if (unit === "mph") return `${signal.value.toFixed(1)} mph`;
  if (unit === "mm3") return `${formatSigned(signal.value, 1)} mm3`;
  if (unit.length === 0) return signal.value.toFixed(1);
  return `${signal.value.toFixed(1)} ${unit}`;
}

function signalSummary(signal: CapabilitySignalSnapshot, units: UnitMode): string {
  return `${signal.label} ${signalDisplayValue(signal, units)}`;
}

type PairSignal = CapabilitySignalSnapshot & {
  composition: Extract<CapabilitySignalSnapshot["composition"], { kind: "pair" }>;
};

type TableRowSignal = CapabilitySignalSnapshot & {
  composition: Extract<CapabilitySignalSnapshot["composition"], { kind: "table_row" }>;
};

function isPairSignal(signal: CapabilitySignalSnapshot): signal is PairSignal {
  return signal.composition.kind === "pair";
}

function isTableRowSignal(signal: CapabilitySignalSnapshot): signal is TableRowSignal {
  return signal.composition.kind === "table_row";
}

function matchesMagic(bytes: Uint8Array, magic: string): boolean {
  if (bytes.length < magic.length) return false;
  for (let i = 0; i < magic.length; i += 1) {
    if (bytes[i] !== magic.charCodeAt(i)) return false;
  }
  return true;
}

interface RecordingFileSource {
  name: string;
  size: number;
}

async function inspectRecordingFile(file: File): Promise<RecordingSummary> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return inspectRecordingBytes({ name: file.name, size: file.size }, bytes);
}

function inspectRecordingBytes(source: RecordingFileSource, bytes: Uint8Array): RecordingSummary {
  const name = source.name;

  if (matchesMagic(bytes, "OBD2REC\u0001")) {
    return parseStructuredRecording(source, bytes, 1);
  }
  if (matchesMagic(bytes, "OBD2REC\u0002")) {
    return parseStructuredRecording(source, bytes, 2);
  }
  if (name.endsWith(".obd2rec.gz")) {
    return {
      name,
      sizeBytes: source.size,
      kind: "compressed",
      detail: "compressed recording",
      preview: [],
      warning: "Compressed .obd2rec.gz inspection needs the Rust reader path. Pick an uncompressed .obd2rec for now.",
    };
  }

  const text = new TextDecoder().decode(bytes);
  if (text.startsWith("# obd2-raw")) {
    return parseRawCapture(source, text);
  }

  return {
    name,
    sizeBytes: source.size,
    kind: "unknown",
    detail: "unknown file",
    preview: text.split(/\r?\n/).slice(0, 16),
    warning: "Expected .obd2rec, .obd2rec.gz, or .obd2raw.",
  };
}

function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

async function inspectRecordingPath(path: string): Promise<RecordingSummary> {
  const bytes = new Uint8Array(await invoke<number[]>("read_recording_file", { path }));
  return inspectRecordingBytes({ name: fileNameFromPath(path), size: bytes.byteLength }, bytes);
}

function parseStructuredRecording(source: RecordingFileSource, bytes: Uint8Array, version: 1 | 2): RecordingSummary {
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
    name: source.name,
    sizeBytes: source.size,
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

function parseRawCapture(source: RecordingFileSource, text: string): RecordingSummary {
  const lines = text.split(/\r?\n/);
  const headerLines = lines.filter((line) => line.startsWith("#"));
  const dataLines = lines.filter((line) => line.trim() !== "" && !line.startsWith("#"));
  const evidencePreview = lines
    .filter((line) => {
      const upper = line.toUpperCase();
      return (
        upper.includes("GM") ||
        upper.includes("6C10F122154201") ||
        upper.includes("6C10F122125101") ||
        upper.includes("6C10F122163D01") ||
        upper.includes("6C10F122163E01") ||
        upper.includes("22154201") ||
        upper.includes("22125101") ||
        upper.includes("22163D01") ||
        upper.includes("22163E01") ||
        upper.includes("19FFFF00") ||
        upper.includes("1992FF00")
      );
    })
    .slice(0, 10);
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
    name: source.name,
    sizeBytes: source.size,
    kind: "raw",
    detail: firstHeader,
    started,
    eventCount: dataLines.length,
    durationMs,
    readEvents,
    writeEvents,
    noteEvents,
    evidencePreview,
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
      <div className="flex h-9 items-center gap-2 border-b border-zinc-800 px-3 text-[11px] font-semibold uppercase text-zinc-400">
        {icon}
        <span>{title}</span>
      </div>
      <div className={bodyClassName}>{children}</div>
    </section>
  );
}

function SessionMenuButton({
  icon,
  label,
  detail,
  onClick,
  disabled = false,
  tone = "default",
}: {
  icon: React.ReactNode;
  label: string;
  detail?: string;
  onClick: () => void;
  disabled?: boolean;
  tone?: "default" | "ok" | "warn" | "crit";
}) {
  const toneClass =
    tone === "ok"
      ? "text-emerald-200 hover:border-emerald-400/50"
      : tone === "warn"
        ? "text-amber-200 hover:border-amber-400/50"
        : tone === "crit"
          ? "text-red-200 hover:border-red-400/50"
          : "text-zinc-200 hover:border-zinc-500";

  return (
    <button
      className={`flex w-full items-start gap-3 rounded-md border border-zinc-800 bg-zinc-950/80 px-3 py-2 text-left transition disabled:cursor-not-allowed disabled:text-zinc-600 disabled:hover:border-zinc-800 ${toneClass}`}
      disabled={disabled}
      onClick={onClick}
      role="menuitem"
      type="button"
    >
      <span className="mt-0.5 flex h-6 w-6 flex-shrink-0 items-center justify-center rounded border border-zinc-800 bg-black/30">
        {icon}
      </span>
      <span className="min-w-0">
        <span className="block text-xs font-semibold">{label}</span>
        {detail ? <span className="mt-0.5 block text-[11px] leading-4 text-zinc-400">{detail}</span> : null}
      </span>
    </button>
  );
}

function Toolbar({
  snapshot,
  unitMode,
  setUnitMode,
  refresh,
  lastRefresh,
  sessionMode,
  selectedRecording,
  replayRunning,
  replayPaused,
  replayError,
  onStartRecording,
  onStopRecording,
  openRecordingFile,
  openRecordingPath,
  setReplayRunning,
  setReplayPaused,
  exitReplay,
}: {
  snapshot: DiagnosticSnapshot;
  unitMode: UnitMode;
  setUnitMode: (mode: UnitMode) => void;
  refresh: () => void;
  lastRefresh: Date;
  sessionMode: SessionMode;
  selectedRecording: RecordingSummary | null;
  replayRunning: boolean;
  replayPaused: boolean;
  replayError: string | null;
  onStartRecording: () => void | Promise<void>;
  onStopRecording: () => void | Promise<void>;
  openRecordingFile: (file: File) => void;
  openRecordingPath: (path: string) => void;
  setReplayRunning: (running: boolean) => void;
  setReplayPaused: (paused: boolean) => void;
  exitReplay: () => void;
}) {
  const [sessionMenuOpen, setSessionMenuOpen] = useState(false);
  const sessionMenuRef = useRef<HTMLDivElement | null>(null);
  const recordingInputRef = useRef<HTMLInputElement | null>(null);
  const isReplay = sessionMode === "replay";
  const isRecording = sessionMode === "recording";
  const modeTitle = isReplay
    ? selectedRecording?.name
      ? `Replay: ${selectedRecording.name}`
      : "Replay"
    : isRecording
      ? "Recording"
      : snapshot.vehicle;
  const modeProtocol = isReplay ? "local playback" : snapshot.protocol;
  const modeCadence = isReplay ? "recording controls" : `${snapshot.poll_ms} ms poll`;
  const modeLabel = isReplay ? "Replay" : isRecording ? "Recording" : "Live";
  const modeButtonClass = isReplay
    ? "border-cyan-400/50 bg-cyan-400/15 text-cyan-100"
    : isRecording
      ? "border-red-400/50 bg-red-500/15 text-red-100"
      : "border-emerald-500/40 bg-emerald-500/10 text-emerald-200";
  const modeIcon = isReplay ? <Radio size={14} /> : isRecording ? <Square size={14} /> : <Cable size={14} />;
  const replayState = selectedRecording == null ? "No file loaded" : replayRunning ? (replayPaused ? "Paused" : "Playing") : "Loaded";

  useEffect(() => {
    if (!sessionMenuOpen) return;

    const closeOnPointerDown = (event: PointerEvent) => {
      if (sessionMenuRef.current?.contains(event.target as Node)) return;
      setSessionMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSessionMenuOpen(false);
    };

    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [sessionMenuOpen]);

  const openRecordingPicker = async () => {
    if (!isTauriRuntime()) {
      recordingInputRef.current?.click();
      return;
    }

    try {
      const defaultPath = await invoke<string>("recordings_directory");
      const selected = await openDialog({
        defaultPath,
        directory: false,
        multiple: false,
        filters: [
          {
            name: "OBD recordings",
            extensions: ["obd2rec", "obd2raw", "gz"],
          },
        ],
      });
      if (typeof selected === "string") {
        openRecordingPath(selected);
        setSessionMenuOpen(false);
      }
    } catch (error) {
      console.error("open recording dialog failed", error);
    }
  };

  const runMenuAction = (action: () => void, close = true) => {
    action();
    if (close) setSessionMenuOpen(false);
  };

  return (
    <header className="sticky top-0 z-10 border-b border-zinc-800 bg-obd-surface px-4 py-3">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex min-w-0 items-center gap-4">
          <div className="flex h-9 w-9 items-center justify-center rounded-md border border-cyan-400/40 bg-cyan-400/10 text-cyan-200">
            <Gauge size={19} />
          </div>
          <div className="min-w-0">
            <div className="truncate text-sm font-semibold text-cyan-200">{modeTitle}</div>
            <div className="mt-0.5 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-zinc-400">
              <span>VIN {snapshot.vin}</span>
              <span>{modeProtocol}</span>
              <span>{modeCadence}</span>
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative" ref={sessionMenuRef}>
            <input
              ref={recordingInputRef}
              aria-label="Open recording file"
              accept=".obd2rec,.obd2rec.gz,.obd2raw"
              className="hidden"
              onChange={(event) => {
                const file = event.currentTarget.files?.[0];
                if (file) openRecordingFile(file);
                event.currentTarget.value = "";
                setSessionMenuOpen(false);
              }}
              type="file"
            />
            <button
              aria-expanded={sessionMenuOpen}
              aria-haspopup="menu"
              aria-label="Session menu"
              className={`inline-flex h-8 min-w-[128px] items-center justify-center gap-2 rounded-md border px-3 text-xs font-semibold ${modeButtonClass}`}
              onClick={() => setSessionMenuOpen((open) => !open)}
              type="button"
            >
              {modeIcon}
              Session: {modeLabel}
              <ChevronDown size={13} />
            </button>
            {sessionMenuOpen ? (
              <div
                className="absolute right-0 z-30 mt-2 w-[320px] rounded-md border border-zinc-700 bg-[#101316] p-3 shadow-2xl shadow-black/40"
                role="menu"
              >
                <div className="mb-3 border-b border-zinc-800 pb-3">
                  <div className="text-[11px] font-semibold uppercase text-zinc-400">Session</div>
                  <div className="mt-1 flex items-center gap-2 text-sm font-semibold text-zinc-100">
                    {modeIcon}
                    {modeLabel}
                  </div>
                  <div className="mt-1 text-[11px] leading-4 text-zinc-400">
                    {isReplay
                      ? selectedRecording?.name ?? replayState
                      : isRecording
                        ? "Capturing live data"
                        : snapshot.connection}
                  </div>
                </div>
                <div className="space-y-2">
                  {isReplay ? (
                    <>
                      <SessionMenuButton
                        icon={<FileText size={14} />}
                        label="Open recording..."
                        detail="Starts in the app recordings folder"
                        onClick={openRecordingPicker}
                      />
                      <SessionMenuButton
                        icon={replayRunning && !replayPaused ? <Pause size={14} /> : <Play size={14} />}
                        label={replayRunning && !replayPaused ? "Pause" : replayRunning ? "Resume" : "Play loaded"}
                        detail={selectedRecording ? replayState : "Choose a recording first"}
                        disabled={selectedRecording == null}
                        tone="ok"
                        onClick={() =>
                          runMenuAction(() => {
                            if (replayRunning) {
                              setReplayPaused(!replayPaused);
                            } else {
                              setReplayRunning(true);
                              setReplayPaused(false);
                            }
                          }, false)
                        }
                      />
                      <SessionMenuButton
                        icon={<Radio size={14} />}
                        label="Exit replay"
                        detail="Return to live diagnostics"
                        onClick={() => runMenuAction(exitReplay)}
                      />
                    </>
                  ) : isRecording ? (
                    <>
                      <SessionMenuButton
                        icon={<Square size={14} />}
                        label="Stop recording"
                        detail="Keep live diagnostics running"
                        tone="crit"
                        onClick={() => runMenuAction(onStopRecording)}
                      />
                      <SessionMenuButton
                        icon={<FileText size={14} />}
                        label="Open recording..."
                        detail="Available after recording stops"
                        disabled
                        onClick={openRecordingPicker}
                      />
                    </>
                  ) : (
                    <>
                      <SessionMenuButton
                        icon={<Square size={14} />}
                        label="Start recording"
                        detail={`${snapshot.poll_ms} ms live polling`}
                        tone="crit"
                        onClick={() => runMenuAction(onStartRecording)}
                      />
                      <SessionMenuButton
                        icon={<FileText size={14} />}
                        label="Open recording..."
                        detail="Starts in the app recordings folder"
                        onClick={openRecordingPicker}
                      />
                    </>
                  )}
                </div>
                {replayError ? (
                  <div className="mt-3 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs leading-5 text-red-200">
                    {replayError}
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
          <button
            className="inline-flex h-8 items-center rounded-md border border-zinc-700 bg-zinc-900 px-3 text-xs font-semibold text-zinc-200 hover:border-zinc-500"
            onClick={() => setUnitMode(unitMode === "us" ? "metric" : "us")}
          >
            {unitMode === "us" ? "US" : "Metric"}
          </button>
          <button
            className="inline-flex h-8 items-center gap-2 rounded-md border border-zinc-700 bg-zinc-900 px-3 text-xs font-semibold text-zinc-200 hover:border-zinc-500"
            onClick={refresh}
          >
            <RotateCcw size={14} />
            Refresh
          </button>
          <div className="hidden min-w-[112px] text-right text-[11px] text-zinc-400 xl:block">
            {lastRefresh.toLocaleTimeString()}
          </div>
        </div>
      </div>
    </header>
  );
}

function tabButtonId(tab: TabId): string {
  return `category-tab-${tab}`;
}

function tabPanelId(tab: TabId): string {
  return `category-panel-${tab}`;
}

function CategoryRail({
  tabs,
  activeTab,
  onSelect,
}: {
  tabs: CategoryTab[];
  activeTab: TabId;
  onSelect: (tab: TabId) => void;
}) {
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  const moveFocus = useCallback(
    (currentTab: TabId, key: string) => {
      const currentIndex = tabs.findIndex((tab) => tab.id === currentTab);
      if (currentIndex < 0) return;

      let nextIndex: number | null = null;
      if (key === "ArrowDown" || key === "ArrowRight") {
        nextIndex = (currentIndex + 1) % tabs.length;
      } else if (key === "ArrowUp" || key === "ArrowLeft") {
        nextIndex = (currentIndex + tabs.length - 1) % tabs.length;
      } else if (key === "Home") {
        nextIndex = 0;
      } else if (key === "End") {
        nextIndex = tabs.length - 1;
      }

      if (nextIndex == null) return;
      const nextTab = tabs[nextIndex]?.id;
      if (!nextTab) return;
      onSelect(nextTab);
      window.requestAnimationFrame(() => tabRefs.current[nextTab]?.focus());
    },
    [onSelect, tabs],
  );

  return (
    <aside
      className={`${panelClass} overflow-hidden lg:sticky lg:top-[84px] lg:h-[calc(100vh-104px)] lg:w-[238px] lg:flex-shrink-0`}
      data-testid="category-rail"
    >
      <div className="border-b border-zinc-800 px-3 py-3">
        <div className="flex min-w-0 items-center gap-2 text-xs font-semibold uppercase text-zinc-400">
          <Radio size={14} />
          <span>Categories</span>
        </div>
      </div>
      <div
        aria-label="Diagnostic categories"
        aria-orientation="vertical"
        className="flex gap-2 overflow-x-auto p-2 lg:min-h-0 lg:flex-1 lg:flex-col lg:overflow-y-auto lg:overflow-x-hidden"
        role="tablist"
      >
        {tabs.map((tab) => {
          const selected = activeTab === tab.id;
          return (
            <button
              aria-selected={selected}
              aria-controls={tabPanelId(tab.id)}
              className={`relative flex min-h-[64px] min-w-[172px] items-center gap-3 rounded-md border px-3 py-2 text-left transition lg:min-w-0 ${
                selected
                  ? "border-cyan-400/50 bg-cyan-400/15 text-cyan-100"
                  : "border-zinc-800 bg-zinc-900/60 text-zinc-400 hover:border-zinc-600 hover:text-zinc-200"
              }`}
              id={tabButtonId(tab.id)}
              key={tab.id}
              onKeyDown={(event) => {
                if (
                  event.key === "ArrowDown" ||
                  event.key === "ArrowRight" ||
                  event.key === "ArrowUp" ||
                  event.key === "ArrowLeft" ||
                  event.key === "Home" ||
                  event.key === "End"
                ) {
                  event.preventDefault();
                  moveFocus(tab.id, event.key);
                }
              }}
              onClick={() => onSelect(tab.id)}
              ref={(node) => {
                tabRefs.current[tab.id] = node;
              }}
              role="tab"
              tabIndex={selected ? 0 : -1}
              type="button"
            >
              {selected ? <span className="absolute bottom-2 left-0 top-2 w-0.5 rounded-r bg-cyan-300" /> : null}
              <span
                className={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md border ${
                  selected
                    ? "border-cyan-400/40 bg-cyan-400/10 text-cyan-100"
                    : "border-zinc-800 bg-black/25 text-zinc-400"
                }`}
              >
                {tab.icon}
              </span>
              <span className="min-w-0">
                <span className="block truncate text-xs font-semibold">{tab.label}</span>
                <span className="mt-1 block truncate text-[11px] text-zinc-400">{tab.summary}</span>
              </span>
            </button>
          );
        })}
      </div>
    </aside>
  );
}

function capabilitySectionSummary(
  section: CapabilitySection,
  sectionSignals: CapabilitySignalSnapshot[],
  snapshot: DiagnosticSnapshot,
  unitMode: UnitMode,
): string {
  if (section.category === "Diagnostics") {
    const alertLabel = snapshot.alerts.length === 1 ? "alert" : "alerts";
    return `${snapshot.dtcs.length} DTC / ${snapshot.alerts.length} ${alertLabel}`;
  }
  if (section.category === "ActiveTests") {
    const tests = capabilitySnapshot(snapshot).active_tests_v2 ?? [];
    if (tests.length === 0) return "none";
    const locked = tests.filter((test) => test.command_profile === "Locked" || !test.actionable).length;
    return locked === tests.length ? "locked" : `${tests.length - locked} ready / ${locked} locked`;
  }
  if (section.category === "Evidence") {
    return `${snapshot.poll_ms} ms snapshot`;
  }
  if (section.category === "Discovery") {
    return `${sectionSignals.length} candidate${sectionSignals.length === 1 ? "" : "s"}`;
  }
  if (sectionSignals.length === 0) return "no live signals";
  if (sectionSignals.length === 1) return signalSummary(sectionSignals[0], unitMode);
  return sectionSignals.slice(0, 2).map((signal) => signalDisplayValue(signal, unitMode)).join(" / ");
}

function StatusStrip({
  snapshot,
  recording,
}: {
  snapshot: DiagnosticSnapshot;
  recording: boolean;
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

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 md:grid-cols-4 xl:grid-cols-8">
      <div className="rounded-md border border-zinc-800 bg-zinc-900/60 px-3 py-2">
        <div className="text-[11px] text-zinc-400">Voltage</div>
        <div className="mt-1 text-lg font-semibold text-zinc-100">{snapshot.voltage.toFixed(1)} V</div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-900/60 px-3 py-2">
        <div className="text-[11px] text-zinc-400">Engine RPM</div>
        <div className="mt-1 text-lg font-semibold text-emerald-300">{snapshot.rpm}</div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-900/60 px-3 py-2">
        <div className="text-[11px] text-zinc-400">Speed</div>
        <div className="mt-1 text-lg font-semibold text-zinc-100">{snapshot.speed_mph} mph</div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-900/60 px-3 py-2">
        <div className="text-[11px] text-zinc-400">Source</div>
        <div
          className={`mt-1 line-clamp-2 text-sm font-medium leading-snug ${
            /error|failed|busy|unavailable|denied/i.test(snapshot.connection)
              ? "text-red-400"
              : "text-cyan-200"
          }`}
          title={snapshot.connection}
        >
          {snapshot.connection}
        </div>
      </div>
      {statuses.map((item) => (
        <div className="rounded-md border border-zinc-800 bg-zinc-900/60 px-3 py-2" key={item.label}>
          <div className="text-[11px] text-zinc-400">{item.label}</div>
          <div className={`mt-1 text-lg font-semibold ${stateClasses[item.state]}`}>{item.value}</div>
        </div>
      ))}
    </div>
  );
}

function GenericActiveTestsPanel({ snapshot }: { snapshot: DiagnosticSnapshot }) {
  const tests = capabilitySnapshot(snapshot).active_tests_v2 ?? [];

  if (tests.length === 0) {
    return (
      <Panel title="Active tests" icon={<SlidersHorizontal size={14} />}>
        <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-3 text-sm text-zinc-400">
          No active tests are exposed by this vehicle profile.
        </div>
      </Panel>
    );
  }

  return (
    <div className="grid gap-3 xl:grid-cols-2">
      {tests.map((test) => {
        const locked = test.command_profile !== "Verified" || !test.actionable;
        const lockReason = locked
          ? `Command profile is ${test.command_profile}; evidence policy ${test.evidence_policy}.`
          : `${test.safety_class} command profile is available.`;
        return (
          <Panel title={test.label} icon={<SlidersHorizontal size={14} />} key={test.key} className="min-h-[420px]">
            <div className={`rounded-md border px-3 py-3 ${
              locked
                ? "border-amber-500/30 bg-amber-500/10"
                : "border-emerald-500/30 bg-emerald-500/10"
            }`}>
              <div className={`flex items-center gap-2 text-sm font-semibold ${locked ? "text-amber-100" : "text-emerald-100"}`}>
                {locked ? <LockKeyhole size={15} /> : <ShieldAlert size={15} />}
                {locked ? "Locked active test" : "Verified active test"}
              </div>
              <div className={`mt-2 text-xs leading-5 ${locked ? "text-amber-200" : "text-emerald-200"}`}>
                {lockReason}
              </div>
            </div>

            <div className="mt-4 grid gap-3 md:grid-cols-2">
              <section className="rounded-md border border-zinc-800 bg-black/25 p-3">
                <div className="text-[11px] font-semibold uppercase text-zinc-400">Command profile</div>
                <div className="mt-2 space-y-1 text-xs leading-5 text-zinc-400">
                  <div>Safety class: {test.safety_class}</div>
                  <div>Timeout: {test.timeout_ms} ms</div>
                  <div>Cancel available: {test.cancel_available ? "yes" : "no"}</div>
                </div>
                <button
                  className="mt-3 inline-flex h-9 items-center gap-2 rounded-md border border-zinc-700 bg-zinc-900 px-3 text-sm font-semibold text-zinc-400 disabled:cursor-not-allowed"
                  disabled
                  type="button"
                >
                  <LockKeyhole size={14} />
                  {locked ? "Command disabled" : "Command UI pending"}
                </button>
              </section>

              <section className="rounded-md border border-zinc-800 bg-black/25 p-3">
                <div className="text-[11px] font-semibold uppercase text-zinc-400">Evidence policy</div>
                <div className="mt-2 space-y-1 text-xs leading-5 text-zinc-400">
                  <div>{test.evidence_policy}</div>
                  <div>{locked ? "No request payload can be constructed from this card." : "Commands must record request and response bytes."}</div>
                </div>
              </section>
            </div>

            <section className="mt-4 rounded-md border border-zinc-800 bg-black/25 p-3">
              <div className="text-[11px] font-semibold uppercase text-zinc-400">Safety gates</div>
              <div className="mt-2 space-y-2">
                {test.preconditions.map((item) => (
                  <div className="rounded-md border border-zinc-800 px-3 py-2" key={item.label}>
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-sm font-semibold text-zinc-200">{item.label}</span>
                      <span className={item.satisfied ? "text-xs font-semibold text-emerald-300" : "text-xs font-semibold text-amber-300"}>
                        {item.satisfied ? "ready" : "blocked"}
                      </span>
                    </div>
                    <div className="mt-1 text-xs leading-5 text-zinc-400">{item.detail}</div>
                  </div>
                ))}
              </div>
            </section>

            {test.last_result ? (
              <div className="mt-4 rounded-md border border-zinc-800 bg-black/25 p-3 text-sm">
                <div className={test.last_result.accepted ? "font-semibold text-emerald-300" : "font-semibold text-amber-300"}>
                  {test.last_result.accepted ? "Accepted" : "Blocked"}: {test.last_result.label}
                </div>
                <div className="mt-2 text-xs leading-5 text-zinc-400">{test.last_result.detail}</div>
              </div>
            ) : null}
          </Panel>
        );
      })}
    </div>
  );
}

function ModuleScanPanel({ modules }: { modules: ModuleScan[] }) {
  return (
    <Panel title="Module scan" icon={<ListTree size={14} />}>
      <table className="w-full border-collapse text-sm">
        <thead>
          <tr className="text-left text-[11px] uppercase text-zinc-400">
            <th className="border-b border-zinc-800 pb-2 font-medium">Module</th>
            <th className="border-b border-zinc-800 pb-2 font-medium">03</th>
            <th className="border-b border-zinc-800 pb-2 font-medium">19 FF</th>
            <th className="border-b border-zinc-800 pb-2 font-medium">19 92</th>
          </tr>
        </thead>
        <tbody>
          {modules.map((module) => (
            <tr className="border-b border-zinc-800 last:border-0" key={module.module}>
              <td className="py-2 font-semibold text-zinc-200">{module.module}</td>
              <td className={scanClass(module.standard)}>{module.standard}</td>
              <td className={scanClass(module.gm_all)}>{module.gm_all}</td>
              <td className={scanClass(module.gm_active)}>{module.gm_active}</td>
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
  if (value === "no data") return "py-2 text-zinc-400";
  if (value === "error") return "py-2 text-red-400";
  if (value.endsWith("dtc")) return "py-2 text-amber-300";
  return "py-2 text-amber-300";
}

function DiagnosticServicesPanel({ services }: { services: DiagnosticServiceSnapshot[] }) {
  if (services.length === 0) return null;
  return (
    <Panel title="Diagnostic services" icon={<FileText size={14} />}>
      <div className="space-y-2">
        {services.map((service) => (
          <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-2 text-sm" key={service.key}>
            <div className="flex items-center justify-between gap-3">
              <span className="font-semibold text-zinc-200">{service.label}</span>
              <span className={service.state === "ok" ? "text-xs font-semibold text-emerald-300" : "text-xs font-semibold text-amber-300"}>
                {service.state}
              </span>
            </div>
            <div className="mt-1 text-xs text-zinc-400">{service.module}</div>
            <div className="mt-1 text-xs leading-5 text-zinc-400">{service.detail}</div>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function DiagnosticsView({
  snapshot,
  commandStatus,
  foregroundPending,
  onRunDiagnostic,
  onRescanVehicle,
  onCancelForeground,
}: {
  snapshot: DiagnosticSnapshot;
  commandStatus: string | null;
  foregroundPending: boolean;
  onRunDiagnostic: () => void;
  onRescanVehicle: () => void;
  onCancelForeground: () => void;
}) {
  const services = capabilitySnapshot(snapshot).diagnostic_services ?? [];
  const foregroundActive =
    snapshot.mode.state === "diagnostic" ||
    (snapshot.mode.state === "discovering" && snapshot.mode.origin === "rescan") ||
    foregroundPending;
  const canCancel = foregroundActive;
  const progress =
    snapshot.mode.state === "diagnostic"
      ? `Phase ${snapshot.mode.phase}/${snapshot.mode.phase_total}; request ${snapshot.mode.step}/${snapshot.mode.total}`
      : snapshot.mode.state === "discovering" && snapshot.mode.origin === "rescan"
        ? `Rescan ${snapshot.mode.step}/${snapshot.mode.total}`
        : null;
  return (
    <div className="grid gap-3 xl:grid-cols-[360px_minmax(0,1fr)_360px]">
      <div className="flex flex-col gap-3">
        <DtcPanel dtcs={snapshot.dtcs} />
        <Panel title="Diagnostic status" icon={<ShieldAlert size={14} />}>
          <div className="space-y-3 text-sm">
            <SettingRow label="MIL" value={snapshot.statuses.find((item) => item.label === "MIL")?.value ?? "--"} />
            <SettingRow label="DTC count" value={snapshot.dtcs.length.toString()} />
            <SettingRow label="Modules" value={snapshot.modules.length.toString()} />
          </div>
          <div className="mt-4 flex flex-wrap gap-2 border-t border-zinc-800 pt-3">
            <button
              className="inline-flex h-8 items-center rounded-md border border-cyan-500/40 bg-cyan-500/10 px-3 text-xs font-semibold text-cyan-100 hover:border-cyan-400 disabled:cursor-not-allowed disabled:opacity-50"
              disabled={foregroundActive}
              onClick={onRunDiagnostic}
              type="button"
            >
              Run diagnostic
            </button>
            <button
              className="inline-flex h-8 items-center rounded-md border border-zinc-700 bg-zinc-900 px-3 text-xs font-semibold text-zinc-200 hover:border-zinc-500 disabled:cursor-not-allowed disabled:opacity-50"
              disabled={foregroundActive}
              onClick={onRescanVehicle}
              type="button"
            >
              Rescan vehicle
            </button>
            {canCancel ? (
              <button
                className="inline-flex h-8 items-center rounded-md border border-amber-500/40 bg-amber-500/10 px-3 text-xs font-semibold text-amber-100 hover:border-amber-400"
                onClick={onCancelForeground}
                type="button"
              >
                Cancel scan
              </button>
            ) : null}
          </div>
          {progress ? <div className="mt-3 text-xs text-cyan-200">{progress}</div> : null}
          {commandStatus ? <div className="mt-3 text-xs text-zinc-400">{commandStatus}</div> : null}
        </Panel>
      </div>
      <div className="flex min-w-0 flex-col gap-3">
        <ModuleScanPanel modules={snapshot.modules} />
        <DiagnosticServicesPanel services={services} />
      </div>
      <AlertsPanel alerts={snapshot.alerts} />
    </div>
  );
}

function EvidenceLine({ evidence }: { evidence: SignalEvidence | undefined }) {
  if (!evidence) return null;
  const statusClass =
    evidence.status === "success"
      ? "text-emerald-300"
      : evidence.status === "cached" || evidence.status === "fallback-gm"
        ? "text-amber-300"
        : "text-zinc-400";
  return (
    <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-2 text-xs leading-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="font-semibold text-zinc-200">{evidence.label}</span>
        <span className={statusClass}>{evidence.status}</span>
      </div>
      <div className="mt-1 text-zinc-400">
        {evidence.source} / {evidence.confidence}
      </div>
      <div className="mt-1 font-mono text-[11px] text-cyan-200">{evidence.request}</div>
      {evidence.response ? <div className="mt-1 font-mono text-[11px] text-zinc-400">response {evidence.response}</div> : null}
    </div>
  );
}

function titleFromKey(key: string): string {
  return key
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase())
    .replace(/\bVgt\b/g, "VGT")
    .replace(/\bMaf\b/g, "MAF")
    .replace(/\bDtc\b/g, "DTC");
}

function GenericSignalReadout({ signal, unitMode }: { signal: CapabilitySignalSnapshot; unitMode: UnitMode }) {
  const tone = runtimeTone(signal);
  return (
    <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate text-[11px] uppercase text-zinc-400" title={signal.label}>
            {signal.label}
          </div>
          <div className={`mt-2 whitespace-nowrap text-xl font-semibold ${stateClasses[tone]}`}>
            {signalDisplayValue(signal, unitMode)}
          </div>
        </div>
        <span
          className={`rounded-sm border px-1.5 py-0.5 text-[10px] font-semibold uppercase ${
            signal.confidence === "Candidate"
              ? "border-amber-500/40 text-amber-300"
              : signal.confidence === "Rejected"
                ? "border-zinc-700 text-zinc-400"
                : "border-zinc-700 text-zinc-400"
          }`}
        >
          {signal.state}
        </span>
      </div>
      <div className="mt-2 flex flex-wrap gap-x-2 gap-y-1 text-[11px] text-zinc-400">
        <span>{signal.module}</span>
        <span>{signal.confidence}</span>
      </div>
      {signal.evidence ? (
        <div className="mt-3">
          <EvidenceLine evidence={signal.evidence} />
        </div>
      ) : null}
    </div>
  );
}

function GenericScalarGrid({
  signals,
  unitMode,
}: {
  signals: CapabilitySignalSnapshot[];
  unitMode: UnitMode;
}) {
  return (
    <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-3">
      {signals.map((signal) => (
        <GenericSignalReadout key={signal.key} signal={signal} unitMode={unitMode} />
      ))}
    </div>
  );
}

function GenericPairPanel({
  groupKey,
  signals,
  unitMode,
}: {
  groupKey: string;
  signals: CapabilitySignalSnapshot[];
  unitMode: UnitMode;
}) {
  const roleOrder: Record<string, number> = { actual: 0, desired: 1, error: 2, delta: 3 };
  const sorted = [...signals].sort((a, b) => {
    const aRole = a.composition.kind === "pair" ? a.composition.role : "";
    const bRole = b.composition.kind === "pair" ? b.composition.role : "";
    return (roleOrder[aRole] ?? 99) - (roleOrder[bRole] ?? 99);
  });
  const title = sorted.find((signal): signal is PairSignal => isPairSignal(signal) && signal.composition.group_label != null)
    ?.composition.group_label ?? titleFromKey(groupKey);

  return (
    <Panel title={title} icon={<Activity size={14} />}>
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
        {sorted.map((signal) => (
          <GenericSignalReadout key={signal.key} signal={signal} unitMode={unitMode} />
        ))}
      </div>
    </Panel>
  );
}

function GenericTablePanel({
  tableKey,
  signals,
  unitMode,
}: {
  tableKey: string;
  signals: CapabilitySignalSnapshot[];
  unitMode: UnitMode;
}) {
  const sorted = [...signals].sort((a, b) => {
    const aIndex = a.composition.kind === "table_row" ? a.composition.row_index : 0;
    const bIndex = b.composition.kind === "table_row" ? b.composition.row_index : 0;
    return aIndex - bIndex;
  });
  const title = sorted.find((signal): signal is TableRowSignal => isTableRowSignal(signal) && signal.composition.table_label != null)
    ?.composition.table_label ?? titleFromKey(tableKey);
  const unit = sorted.find((signal) => signal.unit)?.unit ?? "";

  return (
    <Panel title={title} icon={<Table2 size={14} />}>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[520px] table-fixed border-collapse text-sm">
          <thead>
            <tr>
              <th className="border border-zinc-800 px-2 py-1 text-left text-[11px] font-medium text-zinc-400">
                Signal
              </th>
              {sorted.map((signal) => {
                const row = signal.composition.kind === "table_row" ? signal.composition.row_label : signal.label;
                return (
                  <th
                    className="border border-zinc-800 px-2 py-1 text-center text-[11px] font-medium text-zinc-400"
                    key={signal.key}
                  >
                    {row}
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            <tr>
              <td className="border border-zinc-800 px-2 py-2 text-xs text-zinc-400">{unit}</td>
              {sorted.map((signal) => (
                <td className={`border border-zinc-800 px-2 py-2 text-center font-semibold ${stateClasses[runtimeTone(signal)]}`} key={signal.key}>
                  {signalDisplayValue(signal, unitMode)}
                </td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

function GenericDerivedPanel({
  signals,
  unitMode,
}: {
  signals: CapabilitySignalSnapshot[];
  unitMode: UnitMode;
}) {
  return (
    <Panel title="Derived signals" icon={<Zap size={14} />}>
      <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-3">
        {signals.map((signal) => {
          const formula = signal.composition.kind === "derived" ? signal.composition.formula_key : "derived";
          return (
            <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-3" key={signal.key}>
              <div className="flex items-center justify-between gap-3">
                <span className="text-[11px] uppercase text-zinc-400">{signal.label}</span>
                <span className="font-mono text-[10px] text-zinc-400">{formula}</span>
              </div>
              <div className={`mt-2 text-xl font-semibold ${stateClasses[runtimeTone(signal)]}`}>
                {signalDisplayValue(signal, unitMode)}
              </div>
            </div>
          );
        })}
      </div>
    </Panel>
  );
}

function CapabilitySectionView({
  section,
  signals,
  unitMode,
}: {
  section: CapabilitySection;
  signals: CapabilitySignalSnapshot[];
  unitMode: UnitMode;
}) {
  const sectionSignals = signalsForSection(section, signals);
  const pairs = new Map<string, CapabilitySignalSnapshot[]>();
  const tables = new Map<string, CapabilitySignalSnapshot[]>();
  const derived: CapabilitySignalSnapshot[] = [];
  const scalars: CapabilitySignalSnapshot[] = [];

  for (const signal of sectionSignals) {
    if (signal.composition.kind === "pair") {
      const group = pairs.get(signal.composition.group_key) ?? [];
      group.push(signal);
      pairs.set(signal.composition.group_key, group);
    } else if (signal.composition.kind === "table_row") {
      const group = tables.get(signal.composition.table_key) ?? [];
      group.push(signal);
      tables.set(signal.composition.table_key, group);
    } else if (signal.composition.kind === "derived") {
      derived.push(signal);
    } else {
      scalars.push(signal);
    }
  }

  if (sectionSignals.length === 0) {
    return (
      <Panel title={section.label} icon={capabilitySectionIcon(section.category)}>
        <div className="rounded-md border border-zinc-800 bg-black/25 px-3 py-3 text-sm text-zinc-400">
          No supported signals in this section.
        </div>
      </Panel>
    );
  }

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid={`capability-section-${sectionKey(section.category)}`}>
      {scalars.length > 0 ? (
        <Panel title={section.label} icon={capabilitySectionIcon(section.category)}>
          <GenericScalarGrid signals={scalars} unitMode={unitMode} />
        </Panel>
      ) : null}
      {[...pairs.entries()].map(([groupKey, groupSignals]) => (
        <GenericPairPanel groupKey={groupKey} key={groupKey} signals={groupSignals} unitMode={unitMode} />
      ))}
      {[...tables.entries()].map(([tableKey, tableSignals]) => (
        <GenericTablePanel key={tableKey} tableKey={tableKey} signals={tableSignals} unitMode={unitMode} />
      ))}
      {derived.length > 0 ? <GenericDerivedPanel signals={derived} unitMode={unitMode} /> : null}
    </div>
  );
}

function CapabilityOverviewView({ snapshot, unitMode }: { snapshot: DiagnosticSnapshot; unitMode: UnitMode }) {
  return (
    <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_360px]">
      <TelemetryBoard snapshot={snapshot} unitMode={unitMode} />
      <div className="flex flex-col gap-3">
        <DtcPanel dtcs={snapshot.dtcs} />
        <AlertsPanel alerts={snapshot.alerts} />
      </div>
    </div>
  );
}

function AlertsPanel({ alerts }: { alerts: string[] }) {
  return (
    <Panel title="Alerts" icon={<AlertTriangle size={14} />} className="min-h-[340px]">
      <div className="min-h-[260px] space-y-2 overflow-y-auto pr-1">
        {alerts.length === 0 ? (
          <div className="text-sm text-zinc-400">No active alerts</div>
        ) : (
          alerts.map((alert) => (
            <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-100" key={alert}>
              {alert}
            </div>
          ))
        )}
      </div>
    </Panel>
  );
}

function DtcPanel({ dtcs }: { dtcs: DtcSnapshot[] }) {
  const pendingCount = dtcs.filter((dtc) => dtc.status.includes("pending")).length;
  const currentCount = dtcs.filter((dtc) => dtc.status.includes("current")).length;
  const storedCount = Math.max(0, dtcs.length - pendingCount);

  return (
    <Panel title="DTCs" icon={<FileText size={14} />}>
      {dtcs.length === 0 ? (
        <div className="rounded-md border border-emerald-500/30 bg-emerald-500/10 px-3 py-3 text-sm text-emerald-200">
          No diagnostic codes
        </div>
      ) : (
        <div className="max-h-48 space-y-2 overflow-y-auto pr-1">
          {dtcs.map((dtc) => (
            <div
              className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm"
              key={`${dtc.module}-${dtc.code}-${dtc.status}`}
            >
              <div className="flex items-center justify-between gap-3">
                <span className="font-semibold text-amber-100">{dtc.code}</span>
                <span className="text-xs uppercase text-cyan-200">{dtc.module}</span>
              </div>
              <div className="mt-1 text-xs text-zinc-400">{dtc.status}</div>
              {dtc.description ? <div className="mt-1 text-xs text-zinc-300">{dtc.description}</div> : null}
              {dtc.notes ? <div className="mt-1 text-[11px] text-zinc-400">{dtc.notes}</div> : null}
            </div>
          ))}
        </div>
      )}
      <div className="mt-3 grid grid-cols-1 gap-2 text-xs text-zinc-400 sm:grid-cols-3">
        <div className="rounded-md border border-zinc-800 px-3 py-2">Stored {storedCount}</div>
        <div className="rounded-md border border-zinc-800 px-3 py-2">Pending {pendingCount}</div>
        <div className="rounded-md border border-zinc-800 px-3 py-2">Current {currentCount}</div>
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
      className="flex h-[calc(100vh-144px)] min-h-[560px] flex-col"
      bodyClassName="flex min-h-0 flex-1 p-3"
      testId="raw-snapshot-panel"
    >
      <pre className="min-h-0 flex-1 overflow-auto rounded-md bg-black/25 p-3 text-xs leading-5 text-zinc-400">
        {payload}
      </pre>
    </Panel>
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
          <section className="rounded-md border border-zinc-800 bg-black/25 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-400">Display units</div>
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
            <div className="mt-3 text-xs leading-5 text-zinc-400">
              Pressure and temperature readouts follow this setting. MAF stays in g/s.
            </div>
          </section>

          <section className="rounded-md border border-zinc-800 bg-black/25 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-400">Polling</div>
            <div className="mt-3 space-y-3 text-sm">
              <SettingRow label="Standard PID poll" value={`${snapshot.poll_ms} ms`} />
              <SettingRow label="Enhanced refresh" value="2.5 s live" />
              <SettingRow label="Mode" value="live snapshot" />
            </div>
          </section>

          <section className="rounded-md border border-zinc-800 bg-black/25 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-400">Adapter</div>
            <div className="mt-3 space-y-3 text-sm">
              <SettingRow label="Protocol" value={snapshot.protocol} />
              <SettingRow label="Transport" value="Tauri command boundary" />
              <SettingRow label="Live serial" value="session-owned" tone="ok" />
            </div>
          </section>

          <section className="rounded-md border border-zinc-800 bg-black/25 p-3">
            <div className="text-[11px] font-semibold uppercase text-zinc-400">Diagnostics</div>
            <div className="mt-3 space-y-3 text-sm">
              <SettingRow label="Enhanced DTC service" value="GM Class 2 $19" tone="ok" />
              <SettingRow label="Desired fuel rail" value="GM $22 163D 01" tone="ok" />
              <SettingRow label="Barometer" value="GM $22 1251 01 candidate" tone="warn" />
              <SettingRow label="Desired MAP" value="GM $22 1542 01 candidate" tone="warn" />
              <SettingRow label="Status byte map" value="GM status byte" />
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
        <div className="mt-4 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-200">
          Live serial ownership stays at the Rust session boundary.
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
    <div className="flex items-center justify-between gap-3 border-b border-zinc-800 pb-2 last:border-0 last:pb-0">
      <span className="text-zinc-400">{label}</span>
      <span className={`text-right font-semibold ${valueClass}`}>{value}</span>
    </div>
  );
}

function App() {
  const [snapshot, setSnapshot] = useState<DiagnosticSnapshot>(initialSnapshot);
  const [unitMode, setUnitMode] = useState<UnitMode>("us");
  const [lastRefresh, setLastRefresh] = useState(new Date());
  const [activeTab, setActiveTab] = useState<TabId>("overview");
  const [recording, setRecording] = useState(false);
  const [replayMode, setReplayMode] = useState(false);
  const [replayPaused, setReplayPaused] = useState(false);
  const [replayRunning, setReplayRunning] = useState(false);
  const [selectedRecording, setSelectedRecording] = useState<RecordingSummary | null>(null);
  const [replayError, setReplayError] = useState<string | null>(null);
  const [commandStatus, setCommandStatus] = useState<string | null>(null);
  const [foregroundPending, setForegroundPending] = useState(false);
  const foregroundObserved = useRef(false);
  const foregroundCommandInFlight = useRef(false);

  const refresh = useCallback(async (): Promise<boolean> => {
    if (isTauriRuntime()) {
      try {
        const next = await invoke<DiagnosticSnapshot>("diagnostic_snapshot");
        setSnapshot(next);
        setLastRefresh(new Date());
        return true;
      } catch (error) {
        console.error("diagnostic_snapshot failed", error);
        return false;
      }
    }
    return false;
  }, []);

  useEffect(() => {
    if (!isTauriRuntime() || replayMode) return;
    let cancelled = false;
    let timer: number | undefined;
    const schedule = () => {
      timer = window.setTimeout(async () => {
        await refresh();
        if (!cancelled) schedule();
      }, 500);
    };
    // Start through a zero-delay timer rather than an immediate promise.
    // React StrictMode cleans up its first development-only effect before
    // this timer can issue IPC, so it cannot create a second in-flight
    // snapshot request while still retaining immediate live startup.
    timer = window.setTimeout(async () => {
      await refresh();
      if (!cancelled) schedule();
    }, 0);
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [refresh, replayMode]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const view = activeTab.startsWith("cap:") ? activeTab.slice(4) : activeTab;
    void invoke("set_active_view", { view }).catch((error) => {
      console.error("set_active_view failed", error);
    });
  }, [activeTab]);

  useEffect(() => {
    const foreground =
      snapshot.mode.state === "diagnostic" ||
      (snapshot.mode.state === "discovering" && snapshot.mode.origin === "rescan");
    if (foreground) {
      foregroundObserved.current = true;
      foregroundCommandInFlight.current = false;
      setForegroundPending(false);
    } else if (foregroundObserved.current) {
      foregroundObserved.current = false;
      foregroundCommandInFlight.current = false;
      setForegroundPending(false);
    }
  }, [snapshot.mode]);

  const sessionMode: SessionMode = replayMode ? "replay" : recording ? "recording" : "live";

  const startRecording = useCallback(async () => {
    if (isTauriRuntime()) {
      try {
        await invoke("start_recording");
      } catch (error) {
        setReplayError(error instanceof Error ? error.message : String(error));
        return;
      }
    }
    setReplayMode(false);
    setReplayPaused(false);
    setReplayRunning(false);
    setRecording(true);
  }, []);

  const stopRecording = useCallback(async () => {
    if (isTauriRuntime()) {
      try {
        await invoke("stop_recording");
      } catch (error) {
        setReplayError(error instanceof Error ? error.message : String(error));
        return;
      }
    }
    setRecording(false);
  }, []);

  const submitRunnerCommand = useCallback(async (command: "run_diagnostic" | "rescan_vehicle" | "cancel_foreground") => {
    const startsForeground = command !== "cancel_foreground";
    if (startsForeground && foregroundCommandInFlight.current) return;
    if (startsForeground) foregroundCommandInFlight.current = true;
    if (!isTauriRuntime()) {
      if (startsForeground) foregroundCommandInFlight.current = false;
      setCommandStatus("Runner controls are available only in the desktop application.");
      return;
    }
    try {
      const reply = await invoke<RunnerCommandReply>(command);
      const label = command.replace(/_/g, " ");
      if (reply === "accepted") {
        if (command !== "cancel_foreground") setForegroundPending(true);
        setCommandStatus(`${label}: accepted at the next request boundary.`);
      } else {
        foregroundCommandInFlight.current = false;
        setForegroundPending(false);
        setCommandStatus(`${label}: ${reply.replace(/_/g, " ")}.`);
      }
    } catch (error) {
      foregroundCommandInFlight.current = false;
      setForegroundPending(false);
      setCommandStatus(`${command.replace(/_/g, " ")}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, []);

  const loadRecording = useCallback((loader: Promise<RecordingSummary>) => {
    setReplayError(null);
    setRecording(false);
    setReplayRunning(false);
    setReplayPaused(false);
    loader
      .then((summary) => {
        setSelectedRecording(summary);
        setReplayMode(true);
      })
      .catch((error: unknown) => {
        setSelectedRecording(null);
        setReplayError(error instanceof Error ? error.message : String(error));
      });
  }, []);

  const openRecordingFile = useCallback(
    (file: File) => {
      loadRecording(inspectRecordingFile(file));
    },
    [loadRecording],
  );

  const openRecordingPath = useCallback(
    (path: string) => {
      loadRecording(inspectRecordingPath(path));
    },
    [loadRecording],
  );

  const exitReplay = useCallback(() => {
    setReplayMode(false);
    setReplayPaused(false);
    setReplayRunning(false);
    setActiveTab("overview");
  }, []);

  const tabs = useMemo<CategoryTab[]>(() => {
    const alertLabel = snapshot.alerts.length === 1 ? "alert" : "alerts";
    const dtcAlertSummary = `${snapshot.dtcs.length} DTC / ${snapshot.alerts.length} ${alertLabel}`;

    const signals = capabilitySignals(snapshot);
    const nextTabs: CategoryTab[] = [
      {
        id: "overview",
        label: "Overview",
        summary: dtcAlertSummary,
        icon: <Gauge size={14} />,
      },
    ];
    const seen = new Set<TabId>(["overview"]);

    for (const section of capabilitySections(snapshot)) {
      const sectionSignals = signalsForSection(section, signals);
      let id: TabId;
      if (section.category === "Diagnostics") {
        id = "diagnostics";
      } else if (section.category === "ActiveTests") {
        id = "active";
      } else if (section.category === "Evidence") {
        id = "raw";
      } else if (section.category === "Replay") {
        continue;
      } else if (section.category === "Raw") {
        id = "raw";
      } else if (section.category === "Settings") {
        id = "settings";
      } else {
        id = capabilityTabId(section.category);
      }

      const hasSectionContent =
        section.category === "Diagnostics" ||
        section.category === "Evidence" ||
        (section.category === "ActiveTests" && (capabilitySnapshot(snapshot).active_tests_v2?.length ?? 0) > 0) ||
        sectionSignals.length > 0;
      if (!hasSectionContent || seen.has(id)) continue;
      seen.add(id);
      nextTabs.push({
        id,
        label: section.label,
        summary: capabilitySectionSummary(
          section,
          sectionSignals,
          snapshot,
          unitMode,
        ),
        icon: capabilitySectionIcon(section.category),
      });
    }

    const utilityTabs: CategoryTab[] = [
      {
        id: "diagnostics",
        label: "Diagnostics",
        summary: dtcAlertSummary,
        icon: <ShieldAlert size={14} />,
      },
      {
        id: "raw",
        label: "Raw",
        summary: `${snapshot.poll_ms} ms snapshot`,
        icon: <Database size={14} />,
      },
      {
        id: "settings",
        label: "Settings",
        summary: unitMode === "us" ? "US units" : "metric units",
        icon: <Settings size={14} />,
      },
    ];

    for (const tab of utilityTabs) {
      if (!seen.has(tab.id)) nextTabs.push(tab);
    }

    return nextTabs;
  }, [snapshot, unitMode]);
  const activeTabMeta = tabs.find((tab) => tab.id === activeTab) ?? tabs[0];
  const activeCapabilitySection = capabilitySectionForTab(snapshot, activeTab);

  useEffect(() => {
    if (!tabs.some((tab) => tab.id === activeTab)) {
      setActiveTab("overview");
    }
  }, [activeTab, tabs]);

  return (
    <div className="min-h-screen bg-[#090b0d] text-zinc-100">
      <Toolbar
        snapshot={snapshot}
        unitMode={unitMode}
        setUnitMode={setUnitMode}
        refresh={refresh}
        lastRefresh={lastRefresh}
        sessionMode={sessionMode}
        selectedRecording={selectedRecording}
        replayRunning={replayRunning}
        replayPaused={replayPaused}
        replayError={replayError}
        onStartRecording={startRecording}
        onStopRecording={stopRecording}
        openRecordingFile={openRecordingFile}
        openRecordingPath={openRecordingPath}
        setReplayRunning={setReplayRunning}
        setReplayPaused={setReplayPaused}
        exitReplay={exitReplay}
      />
      <main className="mx-auto flex max-w-[1760px] flex-col gap-3 px-4 py-4">
        <StatusStrip snapshot={snapshot} recording={recording} />
        <div className="flex min-w-0 flex-col gap-3 lg:flex-row lg:items-start">
          <CategoryRail tabs={tabs} activeTab={activeTab} onSelect={setActiveTab} />
          <section
            aria-labelledby={tabButtonId(activeTabMeta.id)}
            className="min-w-0 flex-1"
            id={tabPanelId(activeTabMeta.id)}
            role="tabpanel"
            tabIndex={0}
          >
            {activeTab === "overview" ? (
              <CapabilityOverviewView snapshot={snapshot} unitMode={unitMode} />
            ) : activeCapabilitySection ? (
              <CapabilitySectionView
                section={activeCapabilitySection}
                signals={capabilitySignals(snapshot)}
                unitMode={unitMode}
              />
            ) : activeTab === "raw" ? (
              <RawPanel snapshot={snapshot} />
            ) : activeTab === "settings" ? (
              <SettingsPanel snapshot={snapshot} unitMode={unitMode} setUnitMode={setUnitMode} />
            ) : activeTab === "active" ? (
              <GenericActiveTestsPanel snapshot={snapshot} />
            ) : activeTab === "diagnostics" ? (
              <DiagnosticsView
                snapshot={snapshot}
                commandStatus={commandStatus}
                foregroundPending={foregroundPending}
                onRunDiagnostic={() => void submitRunnerCommand("run_diagnostic")}
                onRescanVehicle={() => void submitRunnerCommand("rescan_vehicle")}
                onCancelForeground={() => void submitRunnerCommand("cancel_foreground")}
              />
            ) : (
              <CapabilityOverviewView snapshot={snapshot} unitMode={unitMode} />
            )}
          </section>
        </div>
      </main>
    </div>
  );
}

export default App;
