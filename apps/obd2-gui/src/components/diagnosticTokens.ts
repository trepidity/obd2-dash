import type { Confidence, SignalRuntimeState, SignalSnapshot, StateKind } from "../types";

export type DiagnosticVisibility = "operational" | "discovery" | "suppressed";

export interface DiagnosticRuntimeToken {
  state: SignalRuntimeState;
  label: string;
  valueClassName: string;
  badgeClassName: string;
  borderClassName: string;
  surfaceClassName: string;
  description: string;
}

export interface DiagnosticTrustToken {
  confidence: Confidence;
  label: string;
  badgeClassName: string;
  textClassName: string;
  borderClassName: string;
  surfaceClassName: string;
  description: string;
}

export interface DiagnosticStateToken {
  state: StateKind;
  label: string;
  textClassName: string;
  badgeClassName: string;
  borderClassName: string;
  surfaceClassName: string;
}

export interface SignalDiagnosticTokens {
  visibility: DiagnosticVisibility;
  runtime: DiagnosticRuntimeToken;
  trust: DiagnosticTrustToken;
}

export const diagnosticLabelClassName = "text-zinc-400";
export const diagnosticSecondaryTextClassName = "text-zinc-400";
export const diagnosticValueClassName = "telemetry-value text-zinc-100";
export const diagnosticMutedValueClassName = "telemetry-value text-zinc-400";

const runtimeTokens: Record<SignalRuntimeState, DiagnosticRuntimeToken> = {
  ok: {
    state: "ok",
    label: "live",
    valueClassName: "telemetry-value text-zinc-100",
    badgeClassName: "border-cyan-400/40 bg-cyan-400/10 text-cyan-200",
    borderClassName: "border-zinc-800",
    surfaceClassName: "bg-zinc-900/60",
    description: "Fresh usable value. This is a transport/runtime state, not a health assertion.",
  },
  warn: {
    state: "warn",
    label: "check",
    valueClassName: "telemetry-value text-amber-300",
    badgeClassName: "border-amber-400/40 bg-amber-400/10 text-amber-300",
    borderClassName: "border-amber-500/30",
    surfaceClassName: "bg-amber-500/10",
    description: "Usable value with a derived consistency warning.",
  },
  waiting: {
    state: "waiting",
    label: "waiting",
    valueClassName: "telemetry-value text-zinc-400",
    badgeClassName: "border-zinc-700 bg-zinc-950/70 text-zinc-400",
    borderClassName: "border-zinc-800",
    surfaceClassName: "bg-zinc-900/40",
    description: "Signal is expected but has not produced a value in this snapshot.",
  },
  cached: {
    state: "cached",
    label: "cached",
    valueClassName: "telemetry-value text-zinc-100",
    badgeClassName: "border-amber-400/40 bg-amber-400/10 text-amber-300",
    borderClassName: "border-amber-500/30",
    surfaceClassName: "bg-amber-500/10",
    description: "Last known value retained. Cached alone is not a live vehicle fault.",
  },
  unsupported: {
    state: "unsupported",
    label: "unsupported",
    valueClassName: "telemetry-value text-zinc-400",
    badgeClassName: "border-zinc-700 bg-zinc-950/70 text-zinc-400",
    borderClassName: "border-zinc-800",
    surfaceClassName: "bg-black/20",
    description: "Vehicle or profile does not expose this signal.",
  },
  error: {
    state: "error",
    label: "error",
    valueClassName: "telemetry-value text-red-400",
    badgeClassName: "border-red-500/40 bg-red-500/10 text-red-300",
    borderClassName: "border-red-500/40",
    surfaceClassName: "bg-red-500/10",
    description: "Read or decode failed. Inspect evidence before trusting the value.",
  },
};

const trustTokens: Record<Confidence, DiagnosticTrustToken> = {
  Candidate: {
    confidence: "Candidate",
    label: "candidate",
    badgeClassName: "border-amber-400/40 bg-amber-400/10 text-amber-300",
    textClassName: "text-amber-300",
    borderClassName: "border-amber-500/30",
    surfaceClassName: "bg-amber-500/10",
    description: "Plausible source pending validation. Show in discovery or evidence lanes.",
  },
  LiveObserved: {
    confidence: "LiveObserved",
    label: "live observed",
    badgeClassName: "border-cyan-400/40 bg-cyan-400/10 text-cyan-200",
    textClassName: "text-cyan-200",
    borderClassName: "border-cyan-500/30",
    surfaceClassName: "bg-cyan-500/10",
    description: "Observed on a live vehicle or derived from live observed values.",
  },
  Community: {
    confidence: "Community",
    label: "community",
    badgeClassName: "border-amber-400/40 bg-amber-400/10 text-amber-300",
    textClassName: "text-amber-300",
    borderClassName: "border-amber-500/30",
    surfaceClassName: "bg-amber-500/10",
    description: "Community or published source. Useful, but not independently verified here.",
  },
  Verified: {
    confidence: "Verified",
    label: "verified",
    badgeClassName: "border-emerald-400/40 bg-emerald-400/10 text-emerald-300",
    textClassName: "text-emerald-300",
    borderClassName: "border-emerald-500/30",
    surfaceClassName: "bg-emerald-500/10",
    description: "Verified source or standard PID with known scaling.",
  },
  Rejected: {
    confidence: "Rejected",
    label: "rejected",
    badgeClassName: "border-zinc-700 bg-zinc-950/70 text-zinc-400",
    textClassName: "text-zinc-400",
    borderClassName: "border-zinc-800",
    surfaceClassName: "bg-black/20",
    description: "Rejected source. Do not present as normal telemetry.",
  },
};

const stateTokens: Record<StateKind, DiagnosticStateToken> = {
  ok: {
    state: "ok",
    label: "ok",
    textClassName: "text-emerald-300",
    badgeClassName: "border-emerald-400/40 bg-emerald-400/10 text-emerald-300",
    borderClassName: "border-emerald-500/30",
    surfaceClassName: "bg-emerald-500/10",
  },
  warn: {
    state: "warn",
    label: "warn",
    textClassName: "text-amber-300",
    badgeClassName: "border-amber-400/40 bg-amber-400/10 text-amber-300",
    borderClassName: "border-amber-500/30",
    surfaceClassName: "bg-amber-500/10",
  },
  crit: {
    state: "crit",
    label: "crit",
    textClassName: "text-red-400",
    badgeClassName: "border-red-500/40 bg-red-500/10 text-red-300",
    borderClassName: "border-red-500/40",
    surfaceClassName: "bg-red-500/10",
  },
  muted: {
    state: "muted",
    label: "muted",
    textClassName: "text-zinc-400",
    badgeClassName: "border-zinc-700 bg-zinc-950/70 text-zinc-400",
    borderClassName: "border-zinc-800",
    surfaceClassName: "bg-black/20",
  },
};

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter((part): part is string => Boolean(part)).join(" ");
}

export function runtimeStateToken(state: SignalRuntimeState): DiagnosticRuntimeToken {
  return runtimeTokens[state];
}

export function trustToken(confidence: Confidence): DiagnosticTrustToken {
  return trustTokens[confidence];
}

export function stateKindToken(state: StateKind): DiagnosticStateToken {
  return stateTokens[state];
}

export function signalVisibility(signal: Pick<SignalSnapshot, "confidence" | "failure_policy">): DiagnosticVisibility {
  if (signal.confidence === "Rejected" || signal.failure_policy === "DoNotPoll") return "suppressed";
  if (signal.confidence === "Candidate") return "discovery";
  return "operational";
}

export function signalDiagnosticTokens(signal: SignalSnapshot): SignalDiagnosticTokens {
  return {
    visibility: signalVisibility(signal),
    runtime: runtimeStateToken(signal.state),
    trust: trustToken(signal.confidence),
  };
}

export function isOperationalSignal(signal: Pick<SignalSnapshot, "confidence" | "failure_policy">): boolean {
  return signalVisibility(signal) === "operational";
}

export function isDiscoverySignal(signal: Pick<SignalSnapshot, "confidence" | "failure_policy">): boolean {
  return signalVisibility(signal) === "discovery";
}
