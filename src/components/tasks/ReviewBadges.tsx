import { ShieldAlert, ShieldCheck, ShieldQuestion, Wrench } from "lucide-react";
import { cn } from "../../lib/utils";
import { REVIEW_LABEL, fixOriginLabel, verdictBadge } from "../../lib/review";
import type { Task } from "../../lib/board";

const BADGE_BASE = "inline-flex flex-shrink-0 items-center gap-1 rounded border px-1.5 py-px font-medium";

/** Badge "Review": distingue i task (e le specifiche) di revisione da quelli di lavoro. */
export function ReviewBadge({ className }: { className?: string }) {
  return (
    <span className={cn(BADGE_BASE, "border-primary/40 text-primary", className)}>
      <ShieldQuestion className="h-3 w-3" aria-hidden="true" />
      {REVIEW_LABEL}
    </span>
  );
}

/** Marcatore "Correzione di <review>" dei task messi in coda da un revisore; niente per gli altri. */
export function FixOriginBadge({ task, tasks, className }: { task: Task; tasks: readonly Task[]; className?: string }) {
  const label = fixOriginLabel(task, tasks);
  if (!label) return null;
  return (
    <span
      className={cn(BADGE_BASE, "min-w-0 max-w-full border-border-strong text-muted-foreground", className)}
      title={label}
    >
      <Wrench className="h-3 w-3 flex-shrink-0" aria-hidden="true" />
      <span className="truncate">{label}</span>
    </span>
  );
}

/** Badge del verdetto di una review conclusa ("Verificato" / "Problemi trovati"); niente per gli altri task. */
export function VerdictBadge({ task, className }: { task: Task; className?: string }) {
  const badge = verdictBadge(task);
  if (!badge) return null;
  const Icon = badge.verdict === "passed" ? ShieldCheck : ShieldAlert;
  return (
    <span className={cn(BADGE_BASE, "border-current/40", badge.className, className)}>
      <Icon className="h-3 w-3" aria-hidden="true" />
      {badge.label}
    </span>
  );
}
