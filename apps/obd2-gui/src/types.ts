export type StateKind = "ok" | "warn" | "crit" | "muted";

export interface StatusValue {
  label: string;
  value: string;
  state: StateKind;
}

export interface CylinderBalance {
  cylinder: number;
  mm3: number;
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

export interface TemperatureSnapshot {
  coolant_f: number;
  intake_air_f: number;
  oil_f: number | null;
  trans_f: number | null;
  ambient_f: number | null;
}

export interface FuelRailSnapshot {
  actual_psi: number;
  desired_psi: number | null;
  delta_psi: number | null;
}

export interface VgtSnapshot {
  actual_pct: number;
  desired_pct: number;
  error_pct: number;
}

export interface ActiveTestDefinition {
  id: "vgt_vane_control";
  label: string;
  locked: boolean;
  lock_reason: string;
  command_profile: string;
  supported_modes: string[];
  safety_notes: string[];
}

export interface ActiveTestPrecondition {
  label: string;
  satisfied: boolean;
  detail: string;
}

export interface ActiveTestResult {
  test_id: "vgt_vane_control";
  label: string;
  accepted: boolean;
  status: string;
  detail: string;
  command: string;
  evidence_path: string | null;
  timestamp: string;
}

export interface VgtActiveTestSnapshot {
  definition: ActiveTestDefinition;
  preconditions: ActiveTestPrecondition[];
  last_result: ActiveTestResult | null;
}

export interface ActiveTestsSnapshot {
  vgt_vane: VgtActiveTestSnapshot;
}

export interface DiagnosticSnapshot {
  vehicle: string;
  vin: string;
  protocol: string;
  connection: string;
  voltage: number;
  rpm: number;
  speed_mph: number;
  poll_ms: number;
  units: string;
  statuses: StatusValue[];
  alerts: string[];
  dtcs: DtcSnapshot[];
  modules: ModuleScan[];
  cylinders: CylinderBalance[];
  vgt: VgtSnapshot;
  fuel_rail: FuelRailSnapshot;
  temperatures: TemperatureSnapshot;
  map_psi: number;
  desired_map_psi: number | null;
  barometric_psi: number | null;
  boost_psi: number;
  maf_g_s: number;
  source_confidence: SignalEvidence[];
  active_tests: ActiveTestsSnapshot;
}
