import { useMemo } from "react";
import { workspaceUpdated } from "../../app/actions/workspaces";
import type { BoardView } from "../../lib/board";
import { needsYou, needsYouKey } from "../../lib/needsYou";
import type { NeedsYouItem } from "../../lib/needsYou";
import type { TaskAgents } from "../../lib/taskAgent";
import AttentionCard from "./AttentionCard";
import PanelFailedCard from "./PanelFailedCard";
import PanelSection from "./PanelSection";
import ProposalCard from "./ProposalCard";
import SettingsProposalCard from "./SettingsProposalCard";

interface NeedsYouSectionProps {
  workspaceId: string;
  board: BoardView;
  agents: TaskAgents;
  onBoard: (board: BoardView) => void;
  /** Riprova un task fallito. Rigetta con l'errore del comando. */
  onRetry: (taskId: string) => Promise<void>;
  onOpenManager: (taskId?: string) => void;
  onFocusTab: (tabId: string) => void;
}

/** Sezione "SERVE A TE" del pannello: proposte, domande, attenzione e falliti in un'unica lista. Assente se vuota. */
export default function NeedsYouSection({
  workspaceId,
  board,
  agents,
  onBoard,
  onRetry,
  onOpenManager,
  onFocusTab,
}: NeedsYouSectionProps) {
  const items = useMemo(() => needsYou(board), [board]);
  if (items.length === 0) return null;

  const render = (item: NeedsYouItem) => {
    switch (item.kind) {
      case "proposal":
        return (
          <ProposalCard
            workspaceId={workspaceId}
            proposal={item.proposal}
            board={board}
            agents={agents}
            onBoard={onBoard}
            goal={item.goal}
            round={item.round}
          />
        );
      case "settings":
        return (
          <SettingsProposalCard
            workspaceId={workspaceId}
            proposal={item.proposal}
            onBoard={onBoard}
            onWorkspace={workspaceUpdated}
          />
        );
      case "attention": {
        const { task } = item;
        return (
          <AttentionCard
            task={task}
            goal={item.goal}
            question={item.question}
            onClick={() => (task.session ? onFocusTab(task.session) : onOpenManager(task.id))}
          />
        );
      }
      case "failed":
        return (
          <PanelFailedCard
            task={item.task}
            goal={item.goal}
            reason={item.reason}
            blocking={item.blocking}
            onRetry={onRetry}
            onOpen={onOpenManager}
          />
        );
    }
  };

  return (
    <PanelSection title="SERVE A TE" count={items.length}>
      <ul className="flex min-w-0 flex-col gap-2">
        {items.map((item) => (
          <li key={needsYouKey(item)} className="min-w-0">
            {render(item)}
          </li>
        ))}
      </ul>
    </PanelSection>
  );
}
