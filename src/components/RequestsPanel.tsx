import { useState } from "react";
import { ChevronDown, ChevronRight, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { cn } from "../lib/utils";
import {
  diffLines,
  repoChanges,
  resolutionLabel,
  splitRequests,
} from "../lib/requests";
import type { UserRequest } from "../lib/requests";

interface Props {
  requests: UserRequest[];
  busy: boolean;
  onAnswer: (id: string, answer: string) => void;
  onDecide: (id: string, approve: boolean, comment: string | null) => void;
  onDismiss: (id: string) => void;
  /** Seleziona il task collegato nel grafo. */
  onSelectTask: (taskId: string) => void;
  onClose: () => void;
}

const MARK_CLASS = {
  added: "bg-status-finished/15 text-status-finished",
  removed: "bg-destructive/15 text-destructive line-through",
  same: "text-muted-foreground",
} as const;
const MARK_SIGN = { added: "+", removed: "−", same: " " } as const;

function DiffRow({ mark, text }: { mark: keyof typeof MARK_CLASS; text: string }) {
  return (
    <li className={cn("flex gap-1.5 rounded px-1.5 py-0.5 font-mono text-[11px]", MARK_CLASS[mark])}>
      <span className="w-2 flex-shrink-0 select-none">{MARK_SIGN[mark]}</span>
      <span className="min-w-0 whitespace-pre-wrap break-words">{text === "" ? " " : text}</span>
    </li>
  );
}

function SettingsDiff({ req }: { req: UserRequest }) {
  const rules = req.proposal?.workspace_rules;
  const changes = repoChanges(req.proposal, req.current);
  return (
    <div className="space-y-2.5">
      {rules !== undefined && (
        <div>
          <p className="mb-1 text-[11px] font-medium">Regole del workspace</p>
          <ul className="space-y-px">
            {diffLines(req.current?.workspace_rules, rules).map((l, i) => (
              <DiffRow key={i} mark={l.mark} text={l.text} />
            ))}
          </ul>
        </div>
      )}
      {changes.map((rc) => (
        <div key={rc.repo}>
          <p className="mb-1 font-mono text-[11px] font-medium">{rc.label}</p>
          <div className="space-y-1.5 border-l pl-2">
            {rc.fields.map((f) => (
              <div key={f.field}>
                <p className="text-[10px] uppercase tracking-[0.06em] text-faint">{f.label}</p>
                {f.type === "list" ? (
                  <ul className="space-y-px">
                    {(f.items ?? []).length === 0 ? (
                      <li className="px-1.5 text-[11px] text-muted-foreground">(nessuno)</li>
                    ) : (
                      f.items!.map((it, i) => <DiffRow key={i} mark={it.mark} text={it.value} />)
                    )}
                  </ul>
                ) : (
                  <p className="flex flex-wrap items-center gap-1.5 font-mono text-[11px]">
                    <span className="rounded bg-destructive/15 px-1.5 text-destructive">{f.before}</span>
                    <span className="text-faint">→</span>
                    <span className="rounded bg-status-finished/15 px-1.5 text-status-finished">
                      {f.after}
                    </span>
                  </p>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
      {rules === undefined && changes.length === 0 && (
        <p className="text-[11px] text-muted-foreground">Nessuna modifica nella proposta.</p>
      )}
    </div>
  );
}

function TaskLink({ id, onSelect }: { id?: string; onSelect: (id: string) => void }) {
  if (!id) return null;
  return (
    <button
      type="button"
      onClick={() => onSelect(id)}
      className="font-mono text-[11px] text-primary hover:underline"
      title="Mostra il task sulla board"
    >
      {id}
    </button>
  );
}

function PendingCard({
  req,
  busy,
  onAnswer,
  onDecide,
  onDismiss,
  onSelectTask,
}: { req: UserRequest } & Omit<Props, "requests" | "onClose">) {
  const [text, setText] = useState("");
  const [comment, setComment] = useState("");
  const isQuestion = req.kind === "question";
  return (
    <div className="space-y-2 rounded-md border border-primary/40 bg-surface-2/40 p-2.5">
      <div className="flex items-center gap-2 text-[10px] uppercase tracking-[0.08em] text-faint">
        <span>{isQuestion ? "Domanda" : "Proposta di impostazioni"}</span>
        <TaskLink id={req.task} onSelect={onSelectTask} />
      </div>
      {isQuestion ? (
        <>
          <p className="whitespace-pre-wrap break-words text-[13px]">{req.question}</p>
          {(req.options ?? []).length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {req.options!.map((o) => (
                <Button
                  key={o}
                  size="sm"
                  variant="secondary"
                  disabled={busy}
                  onClick={() => onAnswer(req.id, o)}
                  className="h-6 px-2 text-[11px]"
                >
                  {o}
                </Button>
              ))}
            </div>
          )}
          <form
            className="flex gap-1.5"
            onSubmit={(e) => {
              e.preventDefault();
              const a = text.trim();
              if (a) onAnswer(req.id, a);
            }}
          >
            <Input
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="Scrivi una risposta…"
              className="h-7 text-xs"
            />
            <Button type="submit" size="sm" disabled={busy || text.trim() === ""} className="h-7 px-2 text-[11px]">
              Rispondi
            </Button>
          </form>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => onDismiss(req.id)}
            className="h-6 px-2 text-[11px] text-muted-foreground"
          >
            Ignora
          </Button>
        </>
      ) : (
        <>
          {req.reason && <p className="whitespace-pre-wrap break-words text-[13px]">{req.reason}</p>}
          <SettingsDiff req={req} />
          <Input
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="Commento (facoltativo)"
            className="h-7 text-xs"
          />
          <div className="flex gap-1.5">
            <Button
              size="sm"
              disabled={busy}
              onClick={() => onDecide(req.id, true, comment.trim() || null)}
              className="h-7 px-3 text-[11px]"
            >
              Approva
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => onDecide(req.id, false, comment.trim() || null)}
              className="h-7 px-3 text-[11px]"
            >
              Rifiuta
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              onClick={() => onDismiss(req.id)}
              className="ml-auto h-7 px-2 text-[11px] text-muted-foreground"
            >
              Ignora
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

function ResolvedRow({ req }: { req: UserRequest }) {
  const title = req.kind === "question" ? req.question : req.reason || "Proposta di impostazioni";
  return (
    <li className="rounded border px-2 py-1.5 text-[11px]">
      <p className="line-clamp-2 break-words text-muted-foreground">{title}</p>
      <p className="mt-0.5 flex flex-wrap gap-x-1.5">
        <span
          className={cn(
            "font-medium",
            req.status === "approved" || req.status === "answered"
              ? "text-status-finished"
              : req.status === "rejected"
                ? "text-destructive"
                : "text-faint",
          )}
        >
          {resolutionLabel(req)}
        </span>
        {req.answer && <span className="break-words">· {req.answer}</span>}
        {req.comment && <span className="break-words text-muted-foreground">· “{req.comment}”</span>}
      </p>
    </li>
  );
}

/** Pannello laterale "Richieste": domande e proposte dell'orchestratore. */
export default function RequestsPanel({ requests, busy, onClose, ...actions }: Props) {
  const { pending, resolved } = splitRequests(requests);
  const [showRecent, setShowRecent] = useState(false);
  return (
    <aside className="flex w-[380px] flex-shrink-0 flex-col border-l bg-base">
      <div className="flex items-center gap-2 border-b px-3 py-2.5">
        <p className="flex-1 text-[13px] font-semibold">
          Richieste <span className="font-mono text-faint">({pending.length})</span>
        </p>
        <button
          type="button"
          aria-label="Chiudi"
          onClick={onClose}
          className="text-muted-foreground hover:text-foreground [&_svg]:size-4"
        >
          <X />
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-2.5 overflow-y-auto p-3">
        {pending.length === 0 && (
          <p className="text-[12px] text-muted-foreground">Nessuna richiesta in attesa.</p>
        )}
        {pending.map((r) => (
          <PendingCard key={r.id} req={r} busy={busy} {...actions} />
        ))}
        {resolved.length > 0 && (
          <div>
            <button
              type="button"
              onClick={() => setShowRecent((v) => !v)}
              className="flex items-center gap-1 text-[11px] font-semibold uppercase tracking-[0.08em] text-faint hover:text-foreground [&_svg]:size-3"
            >
              {showRecent ? <ChevronDown /> : <ChevronRight />}
              Recenti ({resolved.length})
            </button>
            {showRecent && (
              <ul className="mt-1.5 space-y-1.5">
                {resolved.map((r) => (
                  <ResolvedRow key={r.id} req={r} />
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
