import { useState } from "react";
import { Loader2 } from "lucide-react";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { Button } from "./ui/button";
import { api } from "../lib/tauri";
import type { OutdatedPlugin, PluginUpdateResult } from "../lib/tauri";

type Phase = "ask" | "updating" | "failed" | "error";

interface Props {
  /** Plugin da aggiornare; lista vuota = dialog chiuso. */
  outdated: OutdatedPlugin[];
  onClose: () => void;
  /** Salva sessione e scrollback: le sessioni si riprendono dopo il riavvio. */
  onBeforeRestart: () => Promise<void>;
}

export default function PluginUpdateDialog({ outdated, onClose, onBeforeRestart }: Props) {
  const [phase, setPhase] = useState<Phase>("ask");
  const [results, setResults] = useState<PluginUpdateResult[]>([]);
  const [error, setError] = useState("");

  const agentNames = [...new Set(outdated.map((o) => o.agent_name))].join(", ");
  const failures = results.filter((r) => !r.ok);
  const anyOk = results.some((r) => r.ok);

  const restart = async () => {
    try {
      await onBeforeRestart();
      await relaunch();
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  };

  const update = async () => {
    setPhase("updating");
    try {
      const res = await api.agentPluginsUpdate();
      setResults(res);
      if (res.every((r) => r.ok)) {
        await restart();
      } else {
        setPhase("failed");
      }
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  };

  const updating = phase === "updating";

  return (
    <Dialog
      open={outdated.length > 0}
      onOpenChange={(o) => {
        if (!o && !updating) onClose();
      }}
    >
      <DialogContent
        onEscapeKeyDown={(e) => updating && e.preventDefault()}
        onPointerDownOutside={(e) => updating && e.preventDefault()}
        onInteractOutside={(e) => updating && e.preventDefault()}
        className={updating ? "[&>button:last-child]:hidden" : undefined}
      >
        {phase === "ask" && (
          <>
            <DialogHeader>
              <DialogTitle>Aggiornamento plugin</DialogTitle>
              <DialogDescription>
                Il plugin di {agentNames} va aggiornato per corrispondere a
                questa versione dell'app.
              </DialogDescription>
            </DialogHeader>
            <ul className="flex max-h-48 flex-col gap-1 overflow-auto text-xs">
              {outdated.map((o) => (
                <li key={`${o.agent_id}:${o.env_key}`} className="rounded border px-2 py-1">
                  <span className="font-medium">{o.env_label}</span>
                  <span className="text-muted-foreground">
                    {" "}
                    · {o.agent_name}: {o.installed_version ?? "non versionato"} →{" "}
                    {o.expected_version}
                  </span>
                </li>
              ))}
            </ul>
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-sm font-medium">
              Al termine l'app si riavvia: le sessioni aperte verranno riavviate
              e riprese automaticamente (i worker automatici in corso vengono
              interrotti).
            </p>
            <DialogFooter className="gap-2 sm:gap-0">
              <Button variant="outline" onClick={onClose}>
                Non ora
              </Button>
              <Button onClick={() => void update()}>Aggiorna</Button>
            </DialogFooter>
          </>
        )}

        {updating && (
          <>
            <DialogHeader>
              <DialogTitle>Aggiornamento plugin</DialogTitle>
              <DialogDescription>
                Aggiornamento in corso, può richiedere qualche decina di secondi.
              </DialogDescription>
            </DialogHeader>
            <div className="flex items-center gap-2 text-sm">
              <Loader2 className="h-4 w-4 animate-spin" />
              Aggiornamento…
            </div>
          </>
        )}

        {phase === "failed" && (
          <>
            <DialogHeader>
              <DialogTitle>Aggiornamento incompleto</DialogTitle>
              <DialogDescription>
                {failures.length === 1
                  ? "Un ambiente non è stato aggiornato."
                  : `${failures.length} ambienti non sono stati aggiornati.`}
              </DialogDescription>
            </DialogHeader>
            <div className="flex max-h-64 flex-col gap-2 overflow-auto">
              {failures.map((f) => (
                <div key={`${f.agent_id}:${f.env_key}`} className="text-xs">
                  <p className="font-medium">
                    {f.env_label} · {f.agent_name}
                  </p>
                  <pre className="mt-1 max-h-24 overflow-auto whitespace-pre-wrap rounded bg-muted p-2 font-mono text-[11px]">
                    {f.log}
                  </pre>
                </div>
              ))}
            </div>
            <DialogFooter className="gap-2 sm:gap-0">
              <Button variant="outline" onClick={onClose}>
                Chiudi
              </Button>
              {anyOk && <Button onClick={() => void restart()}>Riavvia</Button>}
            </DialogFooter>
          </>
        )}

        {phase === "error" && (
          <>
            <DialogHeader>
              <DialogTitle>Aggiornamento non riuscito</DialogTitle>
            </DialogHeader>
            <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded bg-muted p-2 font-mono text-[11px]">
              {error}
            </pre>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Chiudi
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
