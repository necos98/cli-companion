import { cn } from "../lib/utils";
import type { Workspace } from "../lib/tauri";

/** Etichetta WIN / WSL del workspace (sidebar, pannello Task della vista Globale). */
export default function EnvChip({ ws }: { ws: Workspace }) {
  const wsl = ws.kind === "wsl";
  return (
    <span
      className={cn(
        "flex-shrink-0 rounded border px-1 py-px font-mono text-[10px] font-medium",
        wsl ? "border-primary/40 text-primary" : "border-border-strong text-muted-foreground",
      )}
    >
      {wsl ? "WSL" : "WIN"}
    </span>
  );
}
