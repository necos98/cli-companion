import { AlertTriangle, Loader2, MessageCircleQuestion } from "lucide-react";
import { PREPARING_LABEL, displayStatus, statusLabel } from "../../lib/board";
import type { Task } from "../../lib/board";
import { runningMeta } from "../../lib/taskPanel";
import { cn } from "@/lib/utils";
import AgentBadge from "./AgentBadge";
import { ReviewBadge } from "./ReviewBadges";
import { TaskE2eBadge } from "./E2eBadges";
import { isReview } from "../../lib/review";
import { isPlan } from "../../lib/plan";
import { PlanBadge } from "./PlanBadge";
import BotBadge from "./BotBadge";
import type { AgentBot } from "../../bindings/AgentBot";
import BotAvatar from "../BotAvatar";
import type { BotImage } from "../../lib/bots";

interface PanelRunningCardProps {
  task: Task;
  now: number;
  /** Bot disponibili: il nome di quello del task. */
  bots: readonly AgentBot[];
  /** Nome del bot (se la funzione e' attiva) del tab del worker. */
  bot?: string;
  /** Nome dell'agente da mostrare (null = niente badge: una sola integrazione, quella del workspace). */
  agent?: string | null;
  /** Immagine del bot (se il suo nome ne ha una). */
  botImage?: BotImage;
  onClick: () => void;
}

/**
 * Card di un task in corso: spinner ambra e "modello · tempo", oppure triangolo
 * arancione e "Richiede attenzione" (bordo evidenziato), oppure fumetto blu e
 * "In attesa di risposta" se il worker ha fatto una domanda. Un solo pulsante.
 */
export default function PanelRunningCard({
  task,
  now,
  bots,
  agent = null,
  bot,
  botImage,
  onClick,
}: PanelRunningCardProps) {
  const shown = displayStatus(task);
  const attention = shown === "attention";
  const question = shown === "question";
  const preparing = Boolean(task.preparing);
  const meta = attention || question ? statusLabel(shown) : preparing ? PREPARING_LABEL : runningMeta(task, now);
  return (
    <button
      type="button"
      onClick={onClick}
      title={bot ? `${bot} · ${task.preparing ?? task.title}` : (task.preparing ?? task.title)}
      className={cn(
        "flex w-full min-w-0 items-start gap-2.5 rounded-lg border bg-surface px-2.5 py-[9px] text-left transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        attention && "border-status-attention/45",
        question && "border-primary/45",
        !attention && !question && "border-border",
      )}
    >
      {attention ? (
        <AlertTriangle className="mt-px h-[15px] w-[15px] flex-shrink-0 text-status-attention" aria-hidden="true" />
      ) : question ? (
        <MessageCircleQuestion className="mt-px h-[15px] w-[15px] flex-shrink-0 text-primary" aria-hidden="true" />
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
          {isPlan(task) && <PlanBadge className="ml-1.5 align-middle text-[10px]" />}
          <BotBadge bots={bots} id={task.bot} className="ml-1.5 align-middle text-[10px]" />
          <TaskE2eBadge task={task} className="ml-1.5 align-middle text-[10px]" />
          <AgentBadge name={agent} className="ml-1.5 align-middle text-[10px]" />
        </span>
        {meta && (
          <span
            className={cn(
              "truncate text-[11px]",
              attention && "text-status-attention",
              question && "text-primary",
              !attention && !question && "text-muted-foreground",
            )}
          >
            {meta}
          </span>
        )}
      </span>
    </button>
  );
}
