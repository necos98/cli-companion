import { useEffect, useState } from "react";
import { Loader2, SquareTerminal, X } from "lucide-react";
import { Button } from "./ui/button";
import { cn } from "../lib/utils";
import {
  ACTION_LABELS,
  allowedActions,
  canEditModel,
  canOpenTerminal,
  columnDotClass,
  columnLabel,
  columnTextClass,
  workerSubStatus,
} from "../lib/board";
import type { Task, TaskAction } from "../lib/board";
import { Input } from "./ui/input";
import { modelError, sanitizeModel } from "../lib/workflow";

interface Props {
  task: Task;
  /** Azione in corso (disabilita i pulsanti). */
  busy: boolean;
  onAction: (action: TaskAction) => void;
  /** Cambia il modello del task (null = predefinito del workspace). */
  onSetModel: (model: string | null) => void;
  onOpenTerminal: (sessionId: string) => void;
  onClose: () => void;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-1 text-[10px] font-semibold uppercase tracking-[0.08em] text-faint">{title}</p>
      {children}
    </div>
  );
}

function Chips({ items }: { items: string[] }) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((s, i) => (
        <span
          key={`${i}:${s}`}
          className="rounded border border-border-strong px-1.5 py-px font-mono text-[11px] text-text-secondary"
        >
          {s}
        </span>
      ))}
    </div>
  );
}

const TEST_COLORS: Record<string, string> = {
  passed: "text-status-finished",
  failed: "text-destructive",
  not_run: "text-muted-foreground",
};

const TEST_LABELS: Record<string, string> = {
  passed: "passato",
  failed: "fallito",
  not_run: "non eseguito",
};

/** Pannello laterale con i dettagli del task selezionato e le azioni consentite. */
export default function TaskDetails({
  task,
  busy,
  onAction,
  onSetModel,
  onOpenTerminal,
  onClose,
}: Props) {
  const [modelDraft, setModelDraft] = useState(task.model ?? "");
  useEffect(() => {
    setModelDraft(task.model ?? "");
  }, [task.id, task.model]);
  const modelProblem = modelError(modelDraft);
  const editableModel = canEditModel(task.status);
  const sub = workerSubStatus(task);
  const actions = allowedActions(task);
  const report = task.report;
  return (
    <aside className="flex w-[340px] flex-shrink-0 flex-col border-l bg-base">
      <div className="flex items-start gap-2 border-b px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <p className="break-words text-[13px] font-semibold">{task.title}</p>
          <p className="truncate font-mono text-[11px] text-faint">{task.id}</p>
          <p className="mt-1 flex items-center gap-1.5 text-[11px]">
            <span className={cn("h-2 w-2 rounded-full", columnDotClass(task.column))} />
            <span className={columnTextClass(task.column)}>{columnLabel(task.column)}</span>
            {sub && <span className={sub.className}>· {sub.text}</span>}
            {task.held && <span className="text-status-attention">· In pausa</span>}
            <span className="text-muted-foreground">· tentativi {task.attempts}</span>
          </p>
        </div>
        <button
          type="button"
          onClick={onClose}
          title="Chiudi"
          aria-label="Chiudi i dettagli"
          className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[5px] text-muted-foreground hover:bg-surface-2 hover:text-foreground [&_svg]:size-[13px]"
        >
          <X />
        </button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3 py-3 text-[12px]">
        <div className="flex flex-wrap gap-1.5">
          {canOpenTerminal(task) && (
            <Button
              size="sm"
              variant="secondary"
              className="h-7 gap-1.5 px-2.5 text-xs"
              onClick={() => onOpenTerminal(task.session!)}
            >
              <SquareTerminal className="!size-3.5" />
              Apri terminale
            </Button>
          )}
          {actions.map((a) => (
            <Button
              key={a}
              size="sm"
              variant={a === "cancel" ? "destructive" : "secondary"}
              disabled={busy}
              className="h-7 px-2.5 text-xs"
              onClick={() => onAction(a)}
            >
              {busy && <Loader2 className="!size-3 animate-spin" />}
              {ACTION_LABELS[a]}
            </Button>
          ))}
        </div>

        {task.note && (
          <Section title="Nota">
            <p className="whitespace-pre-wrap break-words text-status-attention">{task.note}</p>
          </Section>
        )}

        <Section title="Task">
          <pre className="max-h-[220px] overflow-auto whitespace-pre-wrap break-words rounded-md border bg-pane-area p-2 font-mono text-[11px] leading-snug text-text-secondary">
            {task.task}
          </pre>
        </Section>

        {task.depends_on.length > 0 && (
          <Section title="Dipendenze">
            <Chips items={task.depends_on} />
          </Section>
        )}
        {task.touches.length > 0 && (
          <Section title="Aree toccate">
            <Chips items={task.touches} />
          </Section>
        )}

        <Section title="Esecuzione">
          <dl className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-[11px]">
            <dt className="text-muted-foreground">Agente</dt>
            <dd className="font-mono">{task.agent}</dd>
            <dt className="text-muted-foreground">Modello</dt>
            <dd className="font-mono">{task.model || "predefinito del workspace"}</dd>
            <dt className="text-muted-foreground">Isolamento</dt>
            <dd className="font-mono">{task.isolation}</dd>
            {task.permission_mode && (
              <>
                <dt className="text-muted-foreground">Permessi</dt>
                <dd className="font-mono">{task.permission_mode}</dd>
              </>
            )}
            {task.priority !== 0 && (
              <>
                <dt className="text-muted-foreground">Priorità</dt>
                <dd className="font-mono">{task.priority}</dd>
              </>
            )}
          </dl>
          {editableModel && (
            <div className="mt-2">
              <div className="flex gap-1.5">
                <Input
                  value={modelDraft}
                  onChange={(e) => setModelDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                  }}
                  placeholder="predefinito del workspace"
                  aria-label="Modello del task"
                  className="h-7 font-mono text-[11px]"
                />
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={
                    busy ||
                    modelProblem !== null ||
                    sanitizeModel(modelDraft) === (task.model ?? null)
                  }
                  className="h-7 px-2.5 text-xs"
                  onClick={() => onSetModel(sanitizeModel(modelDraft))}
                >
                  Salva
                </Button>
                {task.model && (
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy}
                    className="h-7 px-2.5 text-xs"
                    onClick={() => onSetModel(null)}
                  >
                    Azzera
                  </Button>
                )}
              </div>
              {modelProblem && (
                <p className="mt-1 text-[11px] text-destructive">{modelProblem}</p>
              )}
            </div>
          )}
        </Section>

        {task.worktree && (
          <Section title="Worktree">
            <dl className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-[11px]">
              <dt className="text-muted-foreground">Branch</dt>
              <dd className="break-all font-mono">{task.worktree.branch}</dd>
              <dt className="text-muted-foreground">Base</dt>
              <dd className="break-all font-mono">{task.worktree.base}</dd>
              <dt className="text-muted-foreground">Path</dt>
              <dd className="break-all font-mono">{task.worktree.path}</dd>
            </dl>
          </Section>
        )}

        {report && (
          <Section title={`Report (${report.status})`}>
            <div className="flex flex-col gap-2">
              <p className="whitespace-pre-wrap break-words text-text-secondary">{report.summary}</p>
              {report.commit && (
                <p className="font-mono text-[11px] text-muted-foreground">commit {report.commit}</p>
              )}
              {report.tests && report.tests.length > 0 && (
                <ul className="flex flex-col gap-0.5">
                  {report.tests.map((t, i) => (
                    <li key={i} className="text-[11px]">
                      <span className="font-mono">{t.command}</span>{" "}
                      <span className={TEST_COLORS[t.result] ?? "text-muted-foreground"}>
                        {TEST_LABELS[t.result] ?? t.result}
                      </span>
                      {t.details && (
                        <span className="block whitespace-pre-wrap break-words text-muted-foreground">
                          {t.details}
                        </span>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {report.files_changed && report.files_changed.length > 0 && (
                <div>
                  <p className="mb-1 text-[11px] text-muted-foreground">File modificati</p>
                  <Chips items={report.files_changed} />
                </div>
              )}
              {report.follow_ups && report.follow_ups.length > 0 && (
                <div>
                  <p className="mb-1 text-[11px] text-muted-foreground">Da fare dopo</p>
                  <ul className="list-disc pl-4 text-text-secondary">
                    {report.follow_ups.map((f, i) => (
                      <li key={i} className="break-words">{f}</li>
                    ))}
                  </ul>
                </div>
              )}
              {report.diffstat && (
                <pre className="max-h-[160px] overflow-auto whitespace-pre-wrap break-words rounded-md border bg-pane-area p-2 font-mono text-[10px] leading-snug text-text-secondary">
                  {report.diffstat}
                </pre>
              )}
            </div>
          </Section>
        )}
      </div>
    </aside>
  );
}
