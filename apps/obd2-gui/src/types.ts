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
  stored: string;
  pending: string;
  permanent: string;
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
  modules: ModuleScan[];
  cylinders: CylinderBalance[];
  vgt: VgtSnapshot;
  fuel_rail: FuelRailSnapshot;
  temperatures: TemperatureSnapshot;
  map_psi: number;
  boost_psi: number;
  maf_lb_min: number;
}
