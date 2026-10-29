import { AlertTriangle, Loader2 } from "lucide-react";
import { PREPARING_LABEL, displayStatus } from "../../lib/board";
import type { Task } from "../../lib/board";
import { runningMeta } from "../../lib/taskPanel";
import { cn } from "@/lib/utils";
import { ReviewBadge } from "./ReviewBadges";
import { TaskE2eBadge } from "./E2eBadges";
import { isReview } from "../../lib/review";
import BotAvatar from "../BotAvatar";
import type { BotImage } from "../../lib/bots";

interface PanelRunningCardProps {
  task: Task;
  now: number;
  /** Nome del bot (se la funzione e' attiva) del tab del worker. */
  bot?: string;
  /** Immagine del bot (se il suo nome ne ha una). */
  botImage?: BotImage;
  onClick: () => void;
}

/**
 * Card di un task in corso: spinner ambra e "modello · tempo", oppure triangolo
 * arancione e "Richiede attenzione" (bordo evidenziato). Un solo pulsante.
 */
export default function PanelRunningCard({ task, now, bot, botImage, onClick }: PanelRunningCardProps) {
  const attention = displayStatus(task) === "attention";
  const preparing = Boolean(task.preparing);
  const meta = attention ? "Richiede attenzione" : preparing ? PREPARING_LABEL : runningMeta(task, now);
  return (
    <button
      type="button"
      onClick={onClick}
      title={bot ? `${bot} · ${task.preparing ?? task.title}` : (task.preparing ?? task.title)}
      className={cn(
        "flex w-full min-w-0 items-start gap-2.5 rounded-lg border bg-surface px-2.5 py-[9px] text-left transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        attention ? "border-status-attention/45" : "border-border",
      )}
    >
      {attention ? (
        <AlertTriangle className="mt-px h-[15px] w-[15px] flex-shrink-0 text-status-attention" aria-hidden="true" />
      ) : (
        <Loader2
          className="mt-px h-[15px] w-[15px] flex-shrink-0 animate-spin text-status-running motion-reduce:animate-none"
          aria-hidden="true"
        />
      )}
      <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
        {bot && (
          <span className="flex min-w-0 items-center gap-1.5 text-[11px] font-medium text-text-secondary">
            <BotAvatar name={bot} size={14} image={botImage} />
            <span className="truncate">{bot}</span>
          </span>
        )}
        <span className="line-clamp-2 break-words text-[13px] font-medium">
          {task.title}
          {isReview(task) && <ReviewBadge className="ml-1.5 align-middle text-[10px]" />}
          <TaskE2eBadge task={task} className="ml-1.5 align-middle text-[10px]" />
        </span>
        {meta && (
          <span className={cn("truncate text-[11px]", attention ? "text-status-attention" : "text-muted-foreground")}>
            {meta}
          </span>
        )}
      </span>
    </button>
  );
}
