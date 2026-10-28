import { useState } from "react";
import { AlertTriangle } from "lucide-react";
import type { ListRow, RemoveRow, SettingsRow, ValueRow } from "../../lib/settingsProposal";
import { UNKNOWN_TEXT } from "../../lib/settingsProposal";

/** Righe "prima → dopo" della card di una proposta di impostazioni. */

const REMOVED_CLASS = "text-destructive";
const ADDED_CLASS = "text-status-finished";

/** Una riga del confronto: segno (− / +) e testo che va a capo. */
function DiffLine({ sign, text, className }: { sign: "−" | "+"; text: string; className: string }) {
  return (
    <div className={`flex min-w-0 gap-1.5 text-xs ${className}`}>
      <span aria-hidden="true" className="flex-shrink-0 select-none">
        {sign}
      </span>
      <span className="sr-only">{sign === "−" ? "prima:" : "dopo:"}</span>
      <span className="min-w-0 whitespace-pre-wrap break-words">{text}</span>
    </div>
  );
}

/** Testo lungo: compresso a poche righe, con il pulsante per vederlo intero. */
function CollapsibleDiffLine({ sign, text, className }: { sign: "−" | "+"; text: string; className: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex min-w-0 flex-col items-start gap-0.5">
      <div className={open ? "" : "line-clamp-3 min-w-0 max-w-full"}>
        <DiffLine sign={sign} text={text} className={className} />
      </div>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="text-[11px] text-primary hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        {open ? "Comprimi" : "Mostra tutto"}
      </button>
    </div>
  );
}

function ValueBody({ row }: { row: ValueRow }) {
  const Line = row.long ? CollapsibleDiffLine : DiffLine;
  return (
    <>
      <Line sign="−" text={row.before ?? UNKNOWN_TEXT} className={REMOVED_CLASS} />
      <Line sign="+" text={row.after} className={ADDED_CLASS} />
    </>
  );
}

function ListBody({ row }: { row: ListRow }) {
  if (row.removed.length === 0 && row.added.length === 0) {
    return <div className="text-xs text-muted-foreground">Nessuna differenza</div>;
  }
  return (
    <>
      {row.removed.map((t, i) => (
        <DiffLine key={`r${i}`} sign="−" text={t} className={REMOVED_CLASS} />
      ))}
      {row.added.map((t, i) => (
        <DiffLine key={`a${i}`} sign="+" text={t} className={ADDED_CLASS} />
      ))}
    </>
  );
}

function RemoveBody({ row }: { row: RemoveRow }) {
  return <div className={`text-xs ${REMOVED_CLASS}`}>{row.label}</div>;
}

/** Nota dei campi sensibili: icona di avviso e motivo. */
function SensitiveNote({ note }: { note: string }) {
  return (
    <div className="flex items-start gap-1 text-[11px] text-status-attention">
      <AlertTriangle className="mt-px h-3 w-3 flex-shrink-0" aria-hidden="true" />
      <span className="min-w-0 break-words">Attenzione: {note}</span>
    </div>
  );
}

function RowBody({ row }: { row: SettingsRow }) {
  switch (row.kind) {
    case "value":
      return <ValueBody row={row} />;
    case "list":
      return <ListBody row={row} />;
    case "remove":
      return <RemoveBody row={row} />;
  }
}

function SettingsRowView({ row }: { row: SettingsRow }) {
  const sensitive = row.sensitive !== null;
  return (
    <li
      data-sensitive={sensitive || undefined}
      className={`flex min-w-0 flex-col gap-0.5 rounded-md border px-2 py-1.5 ${
        sensitive ? "border-status-attention/60 bg-status-attention/10" : "border-border bg-base/40"
      }`}
    >
      {row.kind !== "remove" && <span className="break-words text-[11px] font-medium text-text-secondary">{row.label}</span>}
      <RowBody row={row} />
      {row.sensitive && <SensitiveNote note={row.sensitive} />}
    </li>
  );
}

/** Elenco delle righe; un titolo precede ogni gruppo (repository) quando cambia. */
export default function SettingsProposalRows({ rows }: { rows: SettingsRow[] }) {
  return (
    <ul className="flex min-w-0 flex-col gap-1.5">
      {rows.map((row, i) => (
        <GroupedRow key={row.field} row={row} first={i === 0 || rows[i - 1].group !== row.group} />
      ))}
    </ul>
  );
}

function GroupedRow({ row, first }: { row: SettingsRow; first: boolean }) {
  return (
    <>
      {first && row.group && (
        <li
          role="presentation"
          className="mt-1 break-words text-[11px] font-semibold tracking-[0.02em] text-foreground first:mt-0"
        >
          {row.group}
        </li>
      )}
      <SettingsRowView row={row} />
    </>
  );
}
