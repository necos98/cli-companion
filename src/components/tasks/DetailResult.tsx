import { ExternalLink } from "lucide-react";
import { cn } from "../../lib/utils";
import { prUrl } from "../../lib/board";
import type { Task, TaskReport, TaskTestResult } from "../../lib/board";
import { openExternal } from "../../lib/openExternal";
import { reviewFindings } from "../../lib/review";
import type { FindingsList } from "../../lib/review";
import Collapsible from "./Collapsible";

const FINDINGS_TONE: Record<FindingsList["tone"], { box: string; title: string }> = {
  attention: { box: "border-status-attention/50", title: "text-status-attention" },
  neutral: { box: "", title: "text-muted-foreground" },
};

const TEST_MARK: Record<string, { mark: string; sr: string; className: string }> = {
  passed: { mark: "✓", sr: "superato", className: "text-status-finished" },
  failed: { mark: "✕", sr: "fallito", className: "text-destructive" },
  not_run: { mark: "–", sr: "non eseguito", className: "text-muted-foreground" },
};

function TestRow({ test }: { test: TaskTestResult }) {
  const m = TEST_MARK[test.result] ?? { mark: "?", sr: test.result, className: "text-muted-foreground" };
  return (
    <li className="flex min-w-0 items-baseline gap-1.5 text-xs">
      <span className={cn("w-3 flex-shrink-0 text-center font-semibold", m.className)} aria-hidden="true">
        {m.mark}
      </span>
      <span className="sr-only">{m.sr}: </span>
      <span className="min-w-0 break-words">
        <span className="font-mono">{test.command}</span>
        {test.details && <span className="text-muted-foreground"> · {test.details}</span>}
      </span>
    </li>
  );
}

/** Etichetta breve del link della PR: "PR #12" se l'indirizzo la contiene, altrimenti l'indirizzo. */
function prLabel(url: string): string {
  const m = /\/pull\/(\d+)/.exec(url);
  return m ? `PR #${m[1]}` : url;
}

function Body({ report, task }: { report: TaskReport; task: Task }) {
  const pr = prUrl(task);
  const tests = report.tests ?? [];
  const files = report.files_changed ?? [];
  const followUps = report.follow_ups ?? [];
  const findings = reviewFindings(task);
  return (
    <>
      {findings && (
        <div className={cn("flex flex-col gap-1 rounded-md border px-2.5 py-2 text-xs", FINDINGS_TONE[findings.tone].box)}>
          <span className={cn("font-medium", FINDINGS_TONE[findings.tone].title)}>{findings.title}</span>
          <ul className="list-disc pl-4 text-text-secondary">
            {findings.items.map((f, i) => (
              <li key={i} className="break-words">
                {f}
              </li>
            ))}
          </ul>
        </div>
      )}
      {report.summary && <p className="whitespace-pre-line break-words leading-relaxed text-text-secondary">{report.summary}</p>}
      {(report.commit || pr) && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
          {report.commit && (
            <span className="font-mono text-text-secondary">
              <span className="font-sans text-muted-foreground">commit </span>
              {report.commit}
            </span>
          )}
          {pr && (
            <button
              type="button"
              onClick={() => openExternal(pr)}
              title={pr}
              className="flex items-center gap-1 rounded font-mono text-primary hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {prLabel(pr)} <ExternalLink className="h-3 w-3" aria-hidden="true" />
            </button>
          )}
        </div>
      )}
      {tests.length > 0 && (
        <ul aria-label="Test" className="flex flex-col gap-1">
          {tests.map((t, i) => (
            <TestRow key={i} test={t} />
          ))}
        </ul>
      )}
      {files.length > 0 && (
        <Collapsible summary={`File modificati (${files.length})`}>
          <ul className="flex flex-col gap-0.5 font-mono text-[11px] text-text-secondary">
            {files.map((f, i) => (
              <li key={i} className="break-all">
                {f}
              </li>
            ))}
          </ul>
        </Collapsible>
      )}
      {followUps.length > 0 && (
        <div className="flex flex-col gap-1 text-xs">
          <span className="text-muted-foreground">Da fare dopo</span>
          <ul className="list-disc pl-4 text-text-secondary">
            {followUps.map((f, i) => (
              <li key={i} className="break-words">
                {f}
              </li>
            ))}
          </ul>
        </div>
      )}
      {report.diffstat && (
        <Collapsible summary="Diffstat">
          <pre
            tabIndex={0}
            className="overflow-x-auto rounded-md border bg-base px-2.5 py-2 font-mono text-[11px] leading-relaxed text-text-secondary"
          >
            {report.diffstat}
          </pre>
        </Collapsible>
      )}
    </>
  );
}

/** Esito di un task concluso: riepilogo, commit, PR, test, file, follow-up, diffstat. */
export default function DetailResult({ task }: { task: Task }) {
  if (!task.report) return null;
  return (
    <section className="flex min-w-0 flex-col gap-2 rounded-lg border bg-surface p-3">
      <h3 className="text-[11px] font-medium uppercase tracking-[0.08em] text-muted-foreground">Esito</h3>
      <Body report={task.report} task={task} />
    </section>
  );
}
