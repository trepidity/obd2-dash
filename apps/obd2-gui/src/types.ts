export type StateKind = "ok" | "warn" | "crit" | "muted";

export interface StatusValue {
  label: string;
  value: string;
  state: StateKind;
}

export interface ModuleScan {
  module: string;
  standard: string;
  gm_all: string;
  gm_active: string;
}

export interface DtcSnapshot {
  code: string;
  module: string;
  status: string;
  description: string | null;
  notes: string | null;
}

export interface SignalEvidence {
  key: string;
  label: string;
  module: string;
  node: string;
  request: string;
  source: string;
  confidence: string;
  status: string;
  unit: string;
  value: number | null;
  response: string | null;
  notes: string | null;
}

export type SignalCategory =
  | "Powertrain"
  | "Turbo"
  | "Fuel"
  | "Transmission"
  | "Body"
  | "Chassis"
  | "Emissions"
  | "Other";

export type Confidence =
  | "Candidate"
  | "LiveObserved"
  | "Community"
  | "Verified"
  | "Rejected";

export type SignalRuntimeState = "ok" | "waiting" | "cached" | "unsupported" | "error";

export interface SignalRxdSource {
  raw: string;
  bit_width: number | null;
}

export interface SignalSourceFields {
  txd: string;
  rxf: string | null;
  rxd: SignalRxdSource | null;
  raw_mth: string | null;
  source_ref: string | null;
}

export type SignalComposition =
  | { kind: "scalar" }
  | { kind: "pair"; group_key: string; group_label?: string; role: "actual" | "desired" | "error" | "delta" }
  | { kind: "table_row"; table_key: string; table_label?: string; row_index: number; row_label: string }
  | { kind: "derived"; group_key: string; group_label?: string; formula_key: string; input_keys: string[] };

export interface SignalSnapshot {
  key: string;
  label: string;
  category: SignalCategory;
  module: string;
  unit: string;
  value: number | null;
  state: SignalRuntimeState;
  confidence: Confidence;
  provenance: string[];
  source_fields?: SignalSourceFields | null;
  request?: string | null;
  decoder_id?: string | null;
  evidence_policy: string;
  failure_policy: string;
  preferred_over: string | null;
  evidence: SignalEvidence | null;
  composition: SignalComposition;
}

export type CapabilitySectionCategory =
  | SignalCategory
  | "Discovery"
  | "Diagnostics"
  | "ActiveTests"
  | "Evidence"
  | "Replay"
  | "Raw"
  | "Settings";

export interface CapabilitySection {
  id?: string;
  category: CapabilitySectionCategory;
  label: string;
  signal_keys: string[];
  active_test_keys?: string[];
  diagnostic_service_keys?: string[];
  visible: boolean;
}

export interface ActiveTestPrecondition {
  label: string;
  satisfied: boolean;
  detail: string;
}

export interface ActiveTestResult {
  test_id: string;
  label: string;
  accepted: boolean;
  status: string;
  detail: string;
  command: string;
  evidence_path: string | null;
  timestamp: string;
}

export interface ActiveTestSnapshotV2 {
  key: string;
  label: string;
  safety_class: string;
  command_profile: "Locked" | "Verified";
  actionable: boolean;
  lock_reason: string | null;
  supported_modes: string[];
  safety_notes: string[];
  preconditions: ActiveTestPrecondition[];
  timeout_ms?: number;
  cancel_available?: boolean;
  evidence_policy?: string;
  last_result: ActiveTestResult | null;
}

export type RunnerMode =
  | { state: "connecting" | "telemetry" | "shutting_down" }
  | { state: "discovering"; origin: "initial" | "rescan"; step: number; total: number }
  | { state: "diagnostic"; phase: number; phase_total: number; step: number; total: number }
  | { state: "reconnecting"; attempt: number };

export interface CapabilityRuntimeState {
  persistence: "cached" | "pending" | "session_only_no_vin" | "session_only_store_error";
  verification: "ready" | "verifying" | "degraded" | "conservative_fallback";
  remaining: number | null;
}

export interface DiagnosticSnapshot {
  mode: RunnerMode;
  capability_state: CapabilityRuntimeState;
  foreground_result: unknown | null;
  vehicle: string;
  vin: string;
  protocol: string;
  connection: string;
  voltage: number | null;
  rpm: number;
  speed_mph: number;
  poll_ms: number;
  runner_sample_age_ms?: number | null;
  sample_at_unix_ms?: number | null;
  units: string;
  statuses: StatusValue[];
  alerts: string[];
  dtc_scan_complete?: boolean;
  dtcs: DtcSnapshot[];
  modules: ModuleScan[];
  source_confidence: SignalEvidence[];
  signals: SignalSnapshot[];
  capability_sections: CapabilitySection[];
  active_tests_v2: ActiveTestSnapshotV2[];
}
