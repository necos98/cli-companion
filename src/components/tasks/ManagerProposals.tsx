import ProposalCard from "./ProposalCard";
import SettingsProposalCard from "./SettingsProposalCard";
import { pendingProposals } from "../../lib/board";
import type { BoardView } from "../../lib/board";
import type { Workspace } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  board: BoardView;
  onBoard: (board: BoardView) => void;
  onWorkspaceUpdated: (workspace: Workspace) => void;
}

/** Fascia delle proposte pendenti (Task Intake e impostazioni), sopra la vista. */
export default function ManagerProposals({ workspaceId, board, onBoard, onWorkspaceUpdated }: Props) {
  if (pendingProposals(board) === 0) return null;
  return (
    <section
      aria-label="Proposte in attesa"
      className="flex max-h-64 flex-shrink-0 gap-3 overflow-auto border-b px-4 py-3"
    >
      {board.proposals.map((p) => (
        <div key={p.id} className="w-[380px] max-w-full flex-shrink-0">
          <ProposalCard workspaceId={workspaceId} proposal={p} board={board} onBoard={onBoard} />
        </div>
      ))}
      {board.settings_proposals.map((p) => (
        <div key={p.id} className="w-[380px] max-w-full flex-shrink-0">
          <SettingsProposalCard
            workspaceId={workspaceId}
            proposal={p}
            onBoard={onBoard}
            onWorkspace={onWorkspaceUpdated}
          />
        </div>
      ))}
    </section>
  );
}
