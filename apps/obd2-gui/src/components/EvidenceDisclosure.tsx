import { useId, useState } from "react";
import type { ReactNode } from "react";
import type { SignalEvidence, SignalSnapshot } from "../types";
import {
  cx,
  diagnosticLabelClassName,
  diagnosticSecondaryTextClassName,
  runtimeStateToken,
  trustToken,
} from "./diagnosticTokens";
import { formatTelemetryValue, type TelemetryUnitMode } from "./TelemetryBoard";

export interface EvidenceDisclosureProps {
  signal: SignalSnapshot;
  evidence?: SignalEvidence | null;
  unitMode?: TelemetryUnitMode;
  defaultOpen?: boolean;
  className?: string;
  renderValue?: (signal: SignalSnapshot) => ReactNode;
}

export interface EvidenceDetailsProps {
  signal: SignalSnapshot;
  evidence?: SignalEvidence | null;
}

function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  if (children == null || children === "") return null;

  return (
    <div className="grid gap-1 border-b border-zinc-800 py-2 last:border-0 sm:grid-cols-[132px_minmax(0,1fr)]">
      <dt className={cx("text-[11px] font-semibold uppercase", diagnosticLabelClassName)}>{label}</dt>
      <dd className="min-w-0 text-xs leading-5 text-zinc-300">{children}</dd>
    </div>
  );
}

function MonoValue({ children }: { children: ReactNode }) {
  return <span className="font-mono text-[11px] text-cyan-200">{children}</span>;
}

export function EvidenceDetails({ signal, evidence }: EvidenceDetailsProps) {
  const sourceFields = signal.source_fields;
  const provenance = signal.provenance.length > 0 ? signal.provenance.join(" / ") : "no provenance";
  const effectiveEvidence = evidence ?? signal.evidence;

  return (
    <dl className="mt-3 rounded-md border border-zinc-800 bg-black/20 px-3 py-1">
      <DetailRow label="Provenance">
        <span className={trustToken(signal.confidence).textClassName}>{provenance}</span>
      </DetailRow>
      <DetailRow label="Request">
        {signal.request ? <MonoValue>{signal.request}</MonoValue> : effectiveEvidence?.request ? <MonoValue>{effectiveEvidence.request}</MonoValue> : null}
      </DetailRow>
      <DetailRow label="Response">
        {effectiveEvidence?.response ? <MonoValue>{effectiveEvidence.response}</MonoValue> : null}
      </DetailRow>
      <DetailRow label="Source">
        {effectiveEvidence ? (
          <span>
            {effectiveEvidence.source} / {effectiveEvidence.confidence}
          </span>
        ) : null}
      </DetailRow>
      <DetailRow label="Status">{effectiveEvidence?.status}</DetailRow>
      <DetailRow label="Module">{effectiveEvidence?.module ?? signal.module}</DetailRow>
      <DetailRow label="Node">{effectiveEvidence?.node}</DetailRow>
      <DetailRow label="Decoder">{signal.decoder_id}</DetailRow>
      <DetailRow label="Policy">
        <span>
          evidence {signal.evidence_policy}; failure {signal.failure_policy}
        </span>
      </DetailRow>
      <DetailRow label="Preferred Over">{signal.preferred_over}</DetailRow>
      <DetailRow label="TXD">{sourceFields?.txd ? <MonoValue>{sourceFields.txd}</MonoValue> : null}</DetailRow>
      <DetailRow label="RXF">{sourceFields?.rxf ? <MonoValue>{sourceFields.rxf}</MonoValue> : null}</DetailRow>
      <DetailRow label="RXD">{sourceFields?.rxd ? <MonoValue>{sourceFields.rxd.raw}</MonoValue> : null}</DetailRow>
      <DetailRow label="MTH">{sourceFields?.raw_mth ? <MonoValue>{sourceFields.raw_mth}</MonoValue> : null}</DetailRow>
      <DetailRow label="Source Ref">{sourceFields?.source_ref}</DetailRow>
      <DetailRow label="Notes">{effectiveEvidence?.notes}</DetailRow>
    </dl>
  );
}

export function EvidenceDisclosure({
  signal,
  evidence,
  unitMode = "us",
  defaultOpen = false,
  className,
  renderValue,
}: EvidenceDisclosureProps) {
  const [open, setOpen] = useState(defaultOpen);
  const generatedId = useId();
  const detailsId = `evidence-${signal.key}-${generatedId}`;
  const runtime = runtimeStateToken(signal.state);
  const trust = trustToken(signal.confidence);
  const effectiveEvidence = evidence ?? signal.evidence;
  const value = renderValue ? renderValue(signal) : formatTelemetryValue(signal, unitMode);

  return (
    <section className={cx("rounded-md border border-zinc-800 bg-zinc-900/60 px-3 py-3", className)}>
      <button
        aria-controls={detailsId}
        aria-expanded={open}
        className="flex w-full min-w-0 items-start justify-between gap-3 text-left"
        onClick={() => setOpen((next) => !next)}
        type="button"
      >
        <span className="min-w-0">
          <span className={cx("block truncate text-[11px] uppercase", diagnosticLabelClassName)} title={signal.label}>
            {signal.label}
          </span>
          <span className={cx("mt-2 block whitespace-nowrap text-lg font-semibold", runtime.valueClassName)}>{value}</span>
          <span className={cx("mt-2 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px]", diagnosticSecondaryTextClassName)}>
            <span>{signal.module}</span>
            <span className={trust.textClassName}>{trust.label}</span>
            {effectiveEvidence?.source ? <span className="truncate">{effectiveEvidence.source}</span> : null}
          </span>
        </span>
        <span className="flex flex-shrink-0 flex-col items-end gap-1">
          <span className={cx("rounded-sm border px-1.5 py-0.5 text-[10px] font-semibold uppercase", runtime.badgeClassName)}>
            {runtime.label}
          </span>
          <span className={cx("rounded-sm border px-1.5 py-0.5 text-[10px] font-semibold uppercase", trust.badgeClassName)}>
            {trust.label}
          </span>
        </span>
      </button>
      {open ? (
        <div id={detailsId}>
          <EvidenceDetails signal={signal} evidence={effectiveEvidence} />
        </div>
      ) : null}
    </section>
  );
}
