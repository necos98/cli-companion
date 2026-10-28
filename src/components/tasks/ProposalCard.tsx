import { useMemo, useState } from "react";
import { Loader2, Sparkles } from "lucide-react";
import { api } from "../../lib/tauri";
import type { BoardView, Proposal, TaskSpec } from "../../lib/board";
import { ReviewBadge } from "./ReviewBadges";
import { SpecE2eBadge } from "./E2eBadges";
import { isReview, proposalHeading } from "../../lib/review";
import { dependencyLabels, dependsOnText, specExcerpt } from "../../lib/taskPanel";

/**
 * Props della card "Proposta da Task Intake" / "Proposta dal reviewer" (pannello Task e Tasks Manager):
 * titoli, estratto della specifica, dipendenze, modello e Scarta / Accetta.
 * La card chiama `api.proposalDecide` e passa a `onBoard` la board restituita;
 * se la validazione fallisce (es. id nel frattempo occupato) mostra l'errore.
 */
export interface ProposalCardProps {
  workspaceId: string;
  proposal: Proposal;
  /** Board corrente: serve a mostrare i titoli delle dipendenze già in coda. */
  board: BoardView;
  onBoard: (board: BoardView) => void;
}

/** "Dipende da A, B · modello" (le parti assenti si omettono). */
function SpecMeta({ spec, board, siblings }: { spec: TaskSpec; board: BoardView; siblings: TaskSpec[] }) {
  const deps = dependsOnText(dependencyLabels(spec.depends_on, board.tasks, siblings));
  if (!deps && !spec.model) return null;
  return (
    <div className="min-w-0 text-[11px] text-muted-foreground">
      {deps && <span className="break-words text-text-secondary">{deps}</span>}
      {deps && spec.model && " · "}
      {spec.model}
    </div>
  );
}

export default function ProposalCard({ workspaceId, proposal, board, onBoard }: ProposalCardProps) {
  const [pending, setPending] = useState<"accept" | "discard" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const specs = proposal.tasks;
  const heading = useMemo(() => proposalHeading(proposal, board.tasks), [proposal, board.tasks]);
  const single = specs.length === 1 ? specs[0] : null;
  const excerpt = useMemo(() => (single ? specExcerpt(single.task) : ""), [single]);

  const decide = async (accept: boolean) => {
    setError(null);
    setPending(accept ? "accept" : "discard");
    try {
      onBoard(await api.proposalDecide(workspaceId, proposal.id, accept));
    } catch (e) {
      setError(String(e));
      setPending(null);
    }
  };

  return (
    <article
      aria-label={heading.title}
      className="flex min-w-0 flex-col gap-2 rounded-[10px] border border-primary/45 bg-primary/5 p-3"
    >
      <div className="flex items-center gap-1.5 text-[11px] font-medium tracking-[0.04em] text-primary">
        <Sparkles className="h-3 w-3 flex-shrink-0" aria-hidden="true" />
        <span className="uppercase">{heading.title}</span>
      </div>
      {heading.origin && (
        <p className="min-w-0 break-words text-[11px] text-muted-foreground">
          Review: <span className="text-text-secondary">{heading.origin}</span>
        </p>
      )}

      {single ? (
        <>
          <div className="break-words text-sm font-semibold">
            {single.title}
            {isReview(single) && <ReviewBadge className="ml-2 align-middle text-[10px]" />}
            <SpecE2eBadge spec={single} className="ml-2 align-middle text-[10px]" />
          </div>
          {excerpt && (
            <p className="line-clamp-3 break-words text-xs leading-normal text-text-secondary">{excerpt}</p>
          )}
          <SpecMeta spec={single} board={board} siblings={specs} />
        </>
      ) : (
        <>
          <div className="text-sm font-semibold">{specs.length} task</div>
          <ul className="flex min-w-0 flex-col gap-1.5">
            {specs.map((s) => (
              <li key={s.id} className="flex min-w-0 flex-col gap-0.5">
                <span className="break-words text-xs font-medium">
                  {s.title}
                  {isReview(s) && <ReviewBadge className="ml-1.5 align-middle text-[10px]" />}
                  <SpecE2eBadge spec={s} className="ml-1.5 align-middle text-[10px]" />
                </span>
                <SpecMeta spec={s} board={board} siblings={specs} />
              </li>
            ))}
          </ul>
        </>
      )}

      {error && (
        <p role="alert" className="break-words text-[11px] text-destructive">
          {error}
        </p>
      )}

      <div className="mt-0.5 flex gap-2">
        <button
          type="button"
          disabled={pending !== null}
          onClick={() => void decide(false)}
          className="flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md border border-border-strong text-xs text-text-secondary transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
        >
          {pending === "discard" && <Loader2 className="h-3 w-3 animate-spin motion-reduce:animate-none" aria-hidden="true" />}
          Scarta
        </button>
        <button
          type="button"
          disabled={pending !== null}
          onClick={() => void decide(true)}
          className="flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md bg-primary text-xs font-semibold text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
        >
          {pending === "accept" && <Loader2 className="h-3 w-3 animate-spin motion-reduce:animate-none" aria-hidden="true" />}
          Accetta
        </button>
      </div>
    </article>
  );
}
