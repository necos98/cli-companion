import { useCallback, useEffect, useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { cn } from "../lib/utils";

type Phase =
  | "idle"
  | "checking"
  | "available"
  | "uptodate"
  | "downloading"
  | "installing"
  | "error";

function shortNotes(notes: string, max = 220): string {
  const oneLine = notes.replace(/\s+/g, " ").trim();
  return oneLine.length > max ? `${oneLine.slice(0, max)}…` : oneLine;
}

/** Data della release in formato locale; stringa vuota se assente o non valida. */
function formatDate(raw?: string): string {
  if (!raw) return "";
  const d = new Date(raw);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString();
}

interface Props {
  /** Esito dell'ultimo controllo: serve al pallino sul pulsante Impostazioni. */
  onAvailable?: (available: boolean) => void;
}

export default function UpdateCheck({ onAvailable }: Props) {
  const [version, setVersion] = useState<string | null>(null);
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);
  // Percentuale di download; null finché la dimensione totale non è nota.
  const [percent, setPercent] = useState<number | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => undefined);
  }, []);

  const checkNow = useCallback(async (silent: boolean) => {
    setError(null);
    if (!silent) setPhase("checking");
    try {
      const found = await check();
      setUpdate(found);
      setPhase(found ? "available" : "uptodate");
      onAvailable?.(found !== null);
    } catch (e) {
      // Controllo silenzioso: rete assente o nessuna release non fanno rumore.
      if (!silent) {
        setPhase("error");
        setError(String(e));
      }
    }
  }, [onAvailable]);

  // Controllo silenzioso all'avvio: mostra il badge solo se c'è un update.
  useEffect(() => {
    void checkNow(true);
  }, [checkNow]);

  const installNow = async () => {
    if (!update) return;
    setError(null);
    setPercent(null);
    setPhase("downloading");
    let total = 0;
    let done = 0;
    try {
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? 0;
          setPercent(total > 0 ? 0 : null);
        } else if (ev.event === "Progress") {
          done += ev.data.chunkLength;
          if (total > 0) setPercent(Math.min(100, Math.round((done / total) * 100)));
        } else {
          setPercent(100);
          setPhase("installing");
        }
      });
      // Su Windows l'app esce da sola quando parte l'installer: il relaunch
      // serve solo se il processo è ancora vivo (altre piattaforme).
      await relaunch();
    } catch (e) {
      setPhase("error");
      setError(String(e));
    }
  };

  const busy =
    phase === "checking" || phase === "downloading" || phase === "installing";
  const date = formatDate(update?.date);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => void checkNow(false)}
          disabled={busy}
        >
          <RefreshCw className={cn("h-3.5 w-3.5", busy && "animate-spin")} />
          {phase === "checking"
            ? "Controllo…"
            : phase === "downloading"
              ? "Download…"
              : phase === "installing"
                ? "Installazione…"
                : "Controlla aggiornamenti"}
        </Button>
        {version && (
          <span className="text-[11px] text-muted-foreground">v{version}</span>
        )}
        {update && phase !== "uptodate" && (
          <Badge>Nuova: {update.version}</Badge>
        )}
      </div>

      {phase === "uptodate" && (
        <p className="text-xs text-muted-foreground">
          Aggiornato alla versione {version}.
        </p>
      )}

      {update && (phase === "available" || busy) && (
        <div className="flex flex-col gap-2 rounded-lg border bg-card p-2.5">
          <p className="text-xs font-semibold">
            Versione {update.version} disponibile
            {date && (
              <span className="font-normal text-muted-foreground"> · {date}</span>
            )}
          </p>
          {update.body && (
            <p className="text-[11px] text-muted-foreground">
              {shortNotes(update.body)}
            </p>
          )}
          <p className="text-[11px] text-muted-foreground">
            L'app si chiuderà e si riaprirà da sola. I server MCP delle sessioni
            agent aperte verranno riavviati.
          </p>
          {(phase === "downloading" || phase === "installing") && (
            <div className="flex items-center gap-2">
              <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                <div
                  className={cn(
                    "h-full bg-primary transition-[width]",
                    percent === null && "w-1/3 animate-pulse",
                  )}
                  style={percent === null ? undefined : { width: `${percent}%` }}
                />
              </div>
              <span className="w-9 text-right text-[11px] text-muted-foreground">
                {percent === null ? "…" : `${percent}%`}
              </span>
            </div>
          )}
          <div>
            <Button size="sm" onClick={() => void installNow()} disabled={busy}>
              <Download className="h-3.5 w-3.5" />
              Aggiorna e riavvia
            </Button>
          </div>
        </div>
      )}

      {error && <p className="text-[11px] text-destructive">{error}</p>}
    </div>
  );
}
