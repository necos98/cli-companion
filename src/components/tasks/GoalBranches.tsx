import { useEffect, useRef, useState } from "react";
import DetailSection from "./DetailSection";
import { useCommand } from "./useDetailCommand";
import { Button } from "../ui/button";
import type { BoardView } from "../../lib/board";
import { canDeleteBranches, goalBranchRows } from "../../lib/goalDetail";
import type { GoalView } from "../../lib/goals";
import { api } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  goal: GoalView;
  open: boolean;
  onBoard: (board: BoardView) => void;
}

/** Branch del goal per repository; se il goal è abbandonato, "Elimina branch" con conferma in linea. */
export default function GoalBranches({ workspaceId, goal, open, onBoard }: Props) {
  const [confirming, setConfirming] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);
  const { run, busy, error, clearError } = useCommand(onBoard);
  const rows = goalBranchRows(goal);
  const deletable = canDeleteBranches(goal);

  // Il riquadro compare al clic: porta la tastiera lì, così la conferma è la prossima cosa raggiungibile.
  useEffect(() => {
    if (confirming) boxRef.current?.focus();
  }, [confirming]);

  if (rows.length === 0) return null;

  const cancel = () => {
    clearError();
    setConfirming(false);
  };

  return (
    <DetailSection title="Branch">
      <ul className="flex flex-col gap-1">
        {rows.map((r) => (
          <li key={r.repo} className="flex min-w-0 flex-wrap items-baseline gap-x-2 text-xs">
            <span className="text-text-secondary">{r.label}</span>
            <code className="break-all font-mono">{r.branch}</code>
            <span className="text-muted-foreground">da {r.base}</span>
          </li>
        ))}
      </ul>
      {open && (
        <p className="text-xs text-muted-foreground">
          Il lavoro resta su questi branch finché il goal non viene consegnato.
        </p>
      )}
      {deletable && !confirming && (
        <Button type="button" variant="outline" size="sm" onClick={() => setConfirming(true)} className="self-start">
          Elimina branch
        </Button>
      )}
      {deletable && confirming && (
        <div
          ref={boxRef}
          tabIndex={-1}
          role="group"
          aria-label="Conferma l'eliminazione dei branch"
          className="flex flex-col gap-2.5 rounded-md border border-destructive/40 bg-surface px-3 py-3 text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <p className="font-medium">Eliminare i branch del goal? Il lavoro che contengono andrà perso per sempre.</p>
          <ul className="list-disc pl-4 text-text-secondary">
            {rows.map((r) => (
              <li key={r.repo} className="break-all">
                <code className="font-mono">{r.branch}</code> ({r.label})
              </li>
            ))}
          </ul>
          {error && (
            <p role="alert" className="break-words text-destructive">
              {error}
            </p>
          )}
          <div className="flex gap-2">
            <Button
              type="button"
              variant="destructive"
              size="sm"
              disabled={busy}
              onClick={() => void run(() => api.goalDeleteBranches(workspaceId, goal.id))}
            >
              {busy ? "Eliminazione…" : "Conferma eliminazione"}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={cancel}
              className="bg-transparent"
            >
              Annulla
            </Button>
          </div>
        </div>
      )}
    </DetailSection>
  );
}
