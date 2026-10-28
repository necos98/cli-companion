import { useMemo, useState } from "react";
import { Loader2, SlidersHorizontal } from "lucide-react";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import type { BoardView, SettingsProposalView } from "../../lib/board";
import { settingsProposalRows } from "../../lib/settingsProposal";
import SettingsProposalRows from "./SettingsProposalRows";

/**
 * Props della card "Proposta di impostazioni" (pannello Task e Tasks Manager):
 * motivazione, confronto prima → dopo campo per campo e Scarta / Applica.
 * La card chiama `api.settingsProposalDecide`: passa a `onBoard` la board
 * restituita e, se la proposta è stata applicata, a `onWorkspace` il workspace
 * aggiornato; se la rivalidazione fallisce mostra l'errore e la proposta resta.
 */
export interface SettingsProposalCardProps {
  workspaceId: string;
  proposal: SettingsProposalView;
  onBoard: (board: BoardView) => void;
  onWorkspace: (workspace: Workspace) => void;
}

export default function SettingsProposalCard({ workspaceId, proposal, onBoard, onWorkspace }: SettingsProposalCardProps) {
  const [pending, setPending] = useState<"accept" | "discard" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rows = useMemo(() => settingsProposalRows(proposal), [proposal]);

  const decide = async (accept: boolean) => {
    setError(null);
    setPending(accept ? "accept" : "discard");
    try {
      const decision = await api.settingsProposalDecide(workspaceId, proposal.id, accept);
      if (decision.workspace) onWorkspace(decision.workspace);
      onBoard(decision.board);
    } catch (e) {
      setError(String(e));
      setPending(null);
    }
  };

  return (
    <article
      aria-label="Proposta di impostazioni"
      className="flex min-w-0 flex-col gap-2 rounded-[10px] border border-primary/45 bg-primary/5 p-3"
    >
      <div className="flex items-center gap-1.5 text-[11px] font-medium tracking-[0.04em] text-primary">
        <SlidersHorizontal className="h-3 w-3 flex-shrink-0" aria-hidden="true" />
        PROPOSTA DI IMPOSTAZIONI
      </div>

      {proposal.reason && <p className="whitespace-pre-wrap break-words text-xs leading-normal">{proposal.reason}</p>}

      <SettingsProposalRows rows={rows} />

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
          Applica
        </button>
      </div>
    </article>
  );
}
