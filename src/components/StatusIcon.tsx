import { Check, CircleAlert } from "lucide-react";
import type { TabStatus } from "../lib/agents";
import { cn } from "../lib/utils";

/** Indicatore di stato di un terminale: pallino busy/idle, ✓ o ❗ dell'agente. */
export default function StatusIcon({ status }: { status: TabStatus }) {
  if (status.icon === "check") {
    return (
      <Check role="img" aria-label={status.label} className="h-3 w-3 flex-shrink-0 text-status-finished">
        <title>{status.label}</title>
      </Check>
    );
  }
  if (status.icon === "alert") {
    return (
      <CircleAlert
        role="img"
        aria-label={status.label}
        className="h-3 w-3 flex-shrink-0 animate-pulse text-status-attention"
      >
        <title>{status.label}</title>
      </CircleAlert>
    );
  }
  return (
    <span
      role="img"
      aria-label={status.label}
      title={status.label}
      className={cn(
        "h-2 w-2 flex-shrink-0 rounded-full",
        status.tone === "busy" ? "bg-status-running" : "bg-status-finished",
      )}
    />
  );
}
