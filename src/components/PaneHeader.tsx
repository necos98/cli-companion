import { Bot, Maximize2, Minimize2, Pin, PinOff, X } from "lucide-react";
import type { TabStatus } from "../lib/agents";
import { tabBadge, statusWord as workerStatusWord } from "../lib/automation";
import { cn } from "../lib/utils";
import StatusIcon from "./StatusIcon";

interface Props {
  label: string;
  /** Nome del workspace, mostrato come prefisso "workspace / label" nella vista globale. */
  prefix?: string;
  status: TabStatus;
  /** Sessione automatica (worker): stato e tooltip; assente = tab normale. */
  automation?: { role?: "worker" | "orchestrator"; status: string; tooltip: string };
  /** Avvio da cui e' nato il tab (l'orchestratore manuale ha il badge ORCH). */
  launchId?: string;
  /** Il pannello riceve la tastiera. */
  focused?: boolean;
  maximized: boolean;
  /** Fissato nella vista globale. */
  pinned: boolean;
  onTogglePin: () => void;
  onToggleMaximize: () => void;
  onClose: () => void;
  onDragStart: (e: React.DragEvent) => void;
  onDragEnd: () => void;
}

/** Parola di stato accanto al nome, solo per gli stati dei coding agent. */
function statusWord(status: TabStatus): { text: string; className: string } | null {
  if (status.icon === "alert") return { text: "aspetta una risposta", className: "text-status-attention" };
  if (status.icon === "check") return { text: "finito", className: "text-status-finished" };
  return null;
}

const BTN =
  "flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[5px] text-muted-foreground transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

/** Intestazione (30px) di un pannello: trascinabile per riordinare i pannelli. */
export default function PaneHeader({
  label,
  prefix,
  status,
  automation,
  launchId,
  focused = false,
  maximized,
  pinned,
  onTogglePin,
  onToggleMaximize,
  onClose,
  onDragStart,
  onDragEnd,
}: Props) {
  // Per un worker la parola di stato e' quella del task, non quella dell'agente.
  const word = automation ? workerStatusWord(automation.status) : statusWord(status);
  const badge = tabBadge({ automation, launchId });
  const baseTitle = prefix ? `${prefix} / ${label}` : label;
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDoubleClick={onToggleMaximize}
      title={automation ? `${baseTitle}\n${automation.tooltip}` : baseTitle}
      className="flex h-[30px] flex-shrink-0 cursor-grab select-none items-center gap-2 border-b bg-pane-header pl-2.5 pr-1.5 active:cursor-grabbing"
    >
      <span className="flex w-3.5 flex-shrink-0 justify-center">
        <StatusIcon status={status} />
      </span>
      {prefix && (
        <>
          <span className="max-w-[40%] flex-shrink truncate text-xs text-muted-foreground">
            {prefix}
          </span>
          <span className="text-faint" aria-hidden="true">
            /
          </span>
        </>
      )}
      <span
        className={cn(
          "min-w-0 truncate font-mono text-xs font-medium",
          focused ? "text-foreground" : "text-text-secondary",
        )}
      >
        {label}
      </span>
      {badge && (
        <span
          title={badge.title}
          className={cn(
            "flex flex-shrink-0 items-center gap-0.5 rounded-[4px] border px-1 font-mono text-[9px] font-medium leading-[14px] tracking-wide",
            badge.orchestrator ? "border-primary/50 text-primary" : "text-muted-foreground",
          )}
        >
          {badge.orchestrator && <Bot className="h-[10px] w-[10px]" />}
          {badge.text}
        </span>
      )}
      {word && (
        <span className={cn("flex-shrink-0 whitespace-nowrap text-[11px]", word.className)}>
          {word.text}
        </span>
      )}
      <span className="flex-1" />
      <button
        onClick={onTogglePin}
        onDoubleClick={(e) => e.stopPropagation()}
        className={cn(BTN, pinned && "text-primary hover:text-primary")}
        title={pinned ? "Togli dalla vista globale" : "Aggiungi alla vista globale"}
        aria-label={pinned ? "Togli dalla vista globale" : "Aggiungi alla vista globale"}
        aria-pressed={pinned}
      >
        {pinned ? <PinOff className="h-[13px] w-[13px]" /> : <Pin className="h-[13px] w-[13px]" />}
      </button>
      <button
        onClick={onToggleMaximize}
        onDoubleClick={(e) => e.stopPropagation()}
        className={BTN}
        title={maximized ? "Ripristina (Ctrl+Maiusc+M)" : "Massimizza (Ctrl+Maiusc+M)"}
        aria-label={maximized ? "Ripristina" : "Massimizza"}
      >
        {maximized ? (
          <Minimize2 className="h-[13px] w-[13px]" />
        ) : (
          <Maximize2 className="h-[13px] w-[13px]" />
        )}
      </button>
      <button
        onClick={onClose}
        onDoubleClick={(e) => e.stopPropagation()}
        className={BTN}
        title="Chiudi terminale"
        aria-label={`Chiudi ${label}`}
      >
        <X className="h-[13px] w-[13px]" />
      </button>
    </div>
  );
}
