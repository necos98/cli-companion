import { terminalCountText } from "../lib/agents";
import type { LimitGroup, LimitItem } from "../lib/usage";
import { cn } from "../lib/utils";

interface Props {
  terminalCount: number;
  /** Terminali che aspettano una risposta. */
  attentionCount: number;
  /** Plugin agent installati ("Claude Code · Windows, Ubuntu"); vuoto = nascosto. */
  agentsSummary: string;
  /** Limiti di utilizzo per ambiente; vuoto = nascosti. */
  limits?: LimitGroup[];
}

const SEVERITY_CLASS: Record<LimitItem["severity"], string> = {
  normal: "",
  muted: "text-faint",
  warning: "text-status-attention",
  critical: "text-destructive",
};

/** Barra di stato (26px): conteggi a sinistra, limiti e plugin installati a destra. */
export default function StatusBar({
  terminalCount,
  attentionCount,
  agentsSummary,
  limits = [],
}: Props) {
  return (
    <div className="flex h-[26px] flex-shrink-0 items-center gap-4 border-t bg-base px-3 font-mono text-[11px] text-muted-foreground">
      <span>{terminalCountText(terminalCount)}</span>
      {attentionCount > 0 && (
        <span className="text-status-attention">{attentionCount} in attesa di risposta</span>
      )}
      <span className="flex-1" />
      {limits.map((g) => (
        <span key={g.envKey} className="flex flex-shrink-0 items-center gap-1.5 whitespace-nowrap">
          {g.prefix && <span className="text-faint">{g.prefix}</span>}
          {g.items.map((item, i) => (
            <span key={item.key} className="flex items-center gap-1.5">
              {i > 0 && (
                <span className="text-faint" aria-hidden="true">
                  │
                </span>
              )}
              <span title={item.title} className={cn(SEVERITY_CLASS[item.severity])}>
                {item.text}
              </span>
            </span>
          ))}
        </span>
      ))}
      {agentsSummary && <span className="truncate">{agentsSummary}</span>}
    </div>
  );
}
