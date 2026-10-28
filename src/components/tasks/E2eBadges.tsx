import { FlaskConical } from "lucide-react";
import { cn } from "../../lib/utils";
import { e2eBadge, specE2eBadge } from "../../lib/e2e";
import type { E2eBadge } from "../../lib/e2e";
import type { Task, TaskSpec } from "../../lib/board";

const BADGE_BASE = "inline-flex flex-shrink-0 items-center gap-1 rounded border border-current/40 px-1.5 py-px font-medium";

function Badge({ badge, className }: { badge: E2eBadge | null; className?: string }) {
  if (!badge) return null;
  return (
    <span className={cn(BADGE_BASE, badge.className, className)}>
      <FlaskConical className="h-3 w-3" aria-hidden="true" />
      {badge.label}
    </span>
  );
}

/** Badge E2E di un task: "E2E richiesto" se marcato e mai eseguito, altrimenti l'esito dell'ultima esecuzione. */
export function TaskE2eBadge({ task, className }: { task: Task; className?: string }) {
  return <Badge badge={e2eBadge(task)} className={className} />;
}

/** Badge E2E di una specifica in una proposta: solo il flag, non c'è ancora nessuna esecuzione. */
export function SpecE2eBadge({ spec, className }: { spec: TaskSpec; className?: string }) {
  return <Badge badge={specE2eBadge(spec)} className={className} />;
}
