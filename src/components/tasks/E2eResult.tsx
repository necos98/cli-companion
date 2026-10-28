import { cn } from "../../lib/utils";
import type { E2eFailure, E2eRun } from "../../lib/board";
import { E2E_STATUS, e2eSummary } from "../../lib/e2e";
import Collapsible from "./Collapsible";

function FailureRow({ failure }: { failure: E2eFailure }) {
  return (
    <li className="min-w-0 break-words">
      <span className="font-mono">{failure.name}</span>
      {failure.file && <span className="font-mono text-muted-foreground"> · {failure.file}</span>}
      <p className="whitespace-pre-line text-muted-foreground">{failure.message}</p>
    </li>
  );
}

const preClass =
  "overflow-x-auto rounded-md border bg-base px-2.5 py-2 font-mono text-[11px] leading-relaxed text-text-secondary";

/** Esito di un'esecuzione degli E2E: stato e riepilogo; test falliti, errore e coda dell'output a scomparsa. */
export default function E2eResult({ run }: { run: E2eRun }) {
  const status = E2E_STATUS[run.status];
  const summary = e2eSummary(run);
  const failed = run.failed_tests ?? [];
  return (
    <div className="flex min-w-0 flex-col gap-1.5 text-xs">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <span className="text-muted-foreground">E2E</span>
        <span className={cn("font-medium", status.className)}>{status.label}</span>
        {summary && <span className="font-mono text-text-secondary">{summary}</span>}
      </div>
      {failed.length > 0 && (
        <Collapsible summary={`Test falliti (${failed.length})`}>
          <ul className="flex flex-col gap-1.5 text-text-secondary">
            {failed.map((f, i) => (
              <FailureRow key={i} failure={f} />
            ))}
          </ul>
        </Collapsible>
      )}
      {run.error && <p className="break-words text-destructive">{run.error}</p>}
      {run.output_tail && (
        <Collapsible summary="Output">
          <pre tabIndex={0} className={preClass}>
            {run.output_tail}
          </pre>
        </Collapsible>
      )}
    </div>
  );
}
