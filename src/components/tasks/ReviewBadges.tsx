import { ShieldAlert, ShieldCheck, ShieldQuestion } from "lucide-react";
import { cn } from "../../lib/utils";
import { REVIEW_LABEL, verdictBadge } from "../../lib/review";
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
