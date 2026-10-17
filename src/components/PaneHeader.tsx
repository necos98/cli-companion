import { Maximize2, Minimize2, X } from "lucide-react";
import type { TabStatus } from "../lib/agents";
import StatusIcon from "./StatusIcon";

interface Props {
  label: string;
  status: TabStatus;
  maximized: boolean;
  onToggleMaximize: () => void;
  onClose: () => void;
  onDragStart: (e: React.DragEvent) => void;
  onDragEnd: () => void;
}

/** Intestazione sottile di un pannello: trascinabile per riordinare i pannelli. */
export default function PaneHeader({
  label,
  status,
  maximized,
  onToggleMaximize,
  onClose,
  onDragStart,
  onDragEnd,
}: Props) {
  const btn =
    "rounded p-0.5 text-muted-foreground hover:bg-background hover:text-foreground";
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDoubleClick={onToggleMaximize}
      title={label}
      className="flex h-6 flex-shrink-0 cursor-grab select-none items-center gap-1.5 border-b bg-card px-2 active:cursor-grabbing"
    >
      <StatusIcon status={status} />
      <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{label}</span>
      <button
        onClick={onToggleMaximize}
        onDoubleClick={(e) => e.stopPropagation()}
        className={btn}
        title={maximized ? "Ripristina (Ctrl+Maiusc+M)" : "Massimizza (Ctrl+Maiusc+M)"}
        aria-label={maximized ? "Ripristina" : "Massimizza"}
      >
        {maximized ? <Minimize2 className="h-3 w-3" /> : <Maximize2 className="h-3 w-3" />}
      </button>
      <button
        onClick={onClose}
        onDoubleClick={(e) => e.stopPropagation()}
        className={btn}
        title="Chiudi terminale"
        aria-label={`Chiudi ${label}`}
      >
        <X className="h-3 w-3" />
      </button>
    </div>
  );
}
