import { useEffect, useRef, useState } from "react";
import { useCommand } from "./useDetailCommand";
import { Button } from "../ui/button";
import type { BoardView } from "../../lib/board";
import { closeBlocked, goalCloseImpact } from "../../lib/goalDetail";
import type { GoalView } from "../../lib/goals";
import { api } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  goal: GoalView;
  board: BoardView;
  onBoard: (board: BoardView) => void;
}

function TitleList({ heading, items }: { heading: string; items: string[] }) {
  if (items.length === 0) return null;
  return (
    <div className="flex flex-col gap-0.5">
      <span className="font-medium">{heading}</span>
      <ul className="list-disc pl-4 text-text-secondary">
        {items.map((title, i) => (
          <li key={`${i}-${title}`} className="break-words">
            {title}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** "Chiudi goal" con conferma in linea: al primo clic elenca ciò che verrà eliminato. */
export default function GoalCloseBox({ workspaceId, goal, board, onBoard }: Props) {
  const [confirming, setConfirming] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);
  const { run, busy, error, clearError } = useCommand(onBoard);
  const impact = goalCloseImpact(board, goal.id);
  const blocked = closeBlocked(impact);
  const nothing = impact.removed.length === 0 && impact.proposals.length === 0;

  // Il riquadro compare al clic: porta la tastiera lì, così la conferma è la prossima cosa raggiungibile.
  useEffect(() => {
    if (confirming) boxRef.current?.focus();
  }, [confirming]);

  const cancel = () => {
    clearError();
    setConfirming(false);
  };

  if (!confirming) {
    return (
      <Button type="button" variant="outline" size="sm" onClick={() => setConfirming(true)} className="self-start">
        Chiudi goal
      </Button>
    );
  }

  return (
    <div
      ref={boxRef}
      tabIndex={-1}
      role="group"
      aria-label="Conferma la chiusura del goal"
      className="flex flex-col gap-2.5 rounded-md border border-destructive/40 bg-surface px-3 py-3 text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <p className="font-medium">Chiudere il goal “{goal.title}”? Non si può riaprire.</p>
      <TitleList heading="Task che verranno eliminati" items={impact.removed.map((t) => t.title)} />
      <TitleList heading="Proposte che verranno scartate" items={impact.proposals} />
      {nothing && <p className="text-text-secondary">Nessun task in coda o fallito e nessuna proposta da eliminare.</p>}
      {blocked && (
        <div role="alert" className="flex flex-col gap-0.5 text-status-running">
          <span>I task in corso devono finire o essere fermati prima di chiudere il goal:</span>
          <ul className="list-disc pl-4">
            {impact.running.map((t) => (
              <li key={t.id} className="break-words">
                {t.title}
              </li>
            ))}
          </ul>
        </div>
      )}
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
          disabled={blocked || busy}
          onClick={() => void run(() => api.goalClose(workspaceId, goal.id))}
        >
          {busy ? "Chiusura…" : "Conferma chiusura"}
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={cancel} className="bg-transparent">
          Annulla
        </Button>
      </div>
    </div>
  );
}
