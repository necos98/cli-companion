import { terminalCountText } from "../lib/agents";

interface Props {
  terminalCount: number;
  /** Terminali che aspettano una risposta. */
  attentionCount: number;
  /** Plugin agent installati ("Claude Code · Windows, Ubuntu"); vuoto = nascosto. */
  agentsSummary: string;
}

/** Barra di stato (26px): conteggi a sinistra, plugin installati a destra. */
export default function StatusBar({ terminalCount, attentionCount, agentsSummary }: Props) {
  return (
    <div className="flex h-[26px] flex-shrink-0 items-center gap-4 border-t bg-base px-3 font-mono text-[11px] text-muted-foreground">
      <span>{terminalCountText(terminalCount)}</span>
      {attentionCount > 0 && (
        <span className="text-status-attention">{attentionCount} in attesa di risposta</span>
      )}
      <span className="flex-1" />
      {agentsSummary && <span className="truncate">{agentsSummary}</span>}
    </div>
  );
}
