import ProposalCard from "./ProposalCard";
import type { BoardView } from "../../lib/board";

interface Props {
  workspaceId: string;
  board: BoardView;
  onBoard: (board: BoardView) => void;
}

/** Fascia delle proposte pendenti di Task Intake, sopra la vista. */
export default function ManagerProposals({ workspaceId, board, onBoard }: Props) {
  if (board.proposals.length === 0) return null;
  return (
    <section
      aria-label="Proposte di Task Intake"
      className="flex max-h-64 flex-shrink-0 gap-3 overflow-auto border-b px-4 py-3"
    >
      {board.proposals.map((p) => (
        <div key={p.id} className="w-[380px] max-w-full flex-shrink-0">
          <ProposalCard workspaceId={workspaceId} proposal={p} board={board} onBoard={onBoard} />
        </div>
      ))}
    </section>
  );
}
