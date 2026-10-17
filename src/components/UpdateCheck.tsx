import { useCallback, useEffect, useState } from "react";
import { Download, RefreshCw, Rocket } from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { api } from "../lib/tauri";
import type { DownloadResult, UpdateConfig, UpdateStatus } from "../lib/updater";
import { cn } from "../lib/utils";

type Phase =
  | "idle"
  | "checking"
  | "available"
  | "uptodate"
  | "downloading"
  | "downloaded"
  | "error";

function shortNotes(notes: string, max = 220): string {
  const oneLine = notes.replace(/\s+/g, " ").trim();
  return oneLine.length > max ? `${oneLine.slice(0, max)}…` : oneLine;
}

interface Props {
  /** Esito dell'ultimo controllo: serve al pallino sul pulsante Impostazioni. */
  onAvailable?: (available: boolean) => void;
}

export default function UpdateCheck({ onAvailable }: Props) {
  const [config, setConfig] = useState<UpdateConfig | null>(null);
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [download, setDownload] = useState<DownloadResult | null>(null);

  const check = useCallback(async (silent: boolean) => {
    setError(null);
    setInfo(null);
    if (!silent) setPhase("checking");
    try {
      const cfg = await api.getUpdateConfig();
      setConfig(cfg);
      if (!cfg.configured) {
        // Repo non ancora esistente: nessun allarme all'avvio, messaggio chiaro
        // solo se l'utente preme il pulsante.
        if (!silent) {
          setPhase("error");
          setError(cfg.message);
        }
        return;
      }
      const st = await api.checkUpdate();
      setStatus(st);
      setPhase(st.update_available ? "available" : "uptodate");
      onAvailable?.(st.update_available);
    } catch (e) {
      if (!silent) {
        setPhase("error");
        setError(String(e));
      }
    }
  }, [onAvailable]);

  // Controllo silenzioso all'avvio: mostra il badge solo se c'è un update.
  useEffect(() => {
    void check(true);
  }, [check]);

  const downloadNow = async () => {
    setError(null);
    setInfo(null);
    setPhase("downloading");
    try {
      const d = await api.downloadUpdate(null);
      setDownload(d);
      setPhase("downloaded");
      setInfo(d.message);
    } catch (e) {
      setPhase("error");
      setError(String(e));
    }
  };

  const installNow = async (launch: boolean) => {
    if (!download) return;
    setError(null);
    setInfo(null);
    try {
      const r = await api.installUpdate(download.file_path, launch);
      setInfo(r.message);
    } catch (e) {
      setError(String(e));
    }
  };

  const busy = phase === "checking" || phase === "downloading";

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => void check(false)}
          disabled={busy}
        >
          <RefreshCw className={cn("h-3.5 w-3.5", busy && "animate-spin")} />
          {phase === "checking"
            ? "Controllo…"
            : phase === "downloading"
              ? "Download…"
              : "Controlla aggiornamenti"}
        </Button>
        {config && (
          <span className="text-[11px] text-muted-foreground">
            v{config.current_version}
          </span>
        )}
        {phase === "available" && status && (
          <Badge>Nuova: {status.latest_version}</Badge>
        )}
      </div>

      {phase === "uptodate" && status && (
        <p className="text-xs text-muted-foreground">
          Aggiornato alla versione {status.current_version}.
        </p>
      )}

      {phase === "available" && status && (
        <div className="flex flex-col gap-2 rounded-lg border bg-card p-2.5">
          <p className="text-xs font-semibold">
            {status.release_name || `Versione ${status.latest_version}`} disponibile
          </p>
          {status.asset && (
            <p className="text-[11px] text-muted-foreground">
              Installer: {status.asset.name}
            </p>
          )}
          {status.release_notes && (
            <p className="text-[11px] text-muted-foreground">
              {shortNotes(status.release_notes)}
            </p>
          )}
          <div>
            <Button size="sm" onClick={() => void downloadNow()}>
              <Download className="h-3.5 w-3.5" />
              Scarica aggiornamento
            </Button>
          </div>
        </div>
      )}

      {phase === "downloaded" && download && (
        <div className="flex flex-col gap-2 rounded-lg border bg-card p-2.5">
          <div className="flex items-center gap-2">
            <p className="min-w-0 flex-1 truncate text-xs font-semibold">
              {download.file_name}
            </p>
            <Badge variant={download.verified ? "default" : "secondary"}>
              {download.verified ? "verificato" : "non verificato"}
            </Badge>
          </div>
          <div className="flex flex-row gap-1.5">
            <Button size="sm" onClick={() => void installNow(true)}>
              <Rocket className="h-3.5 w-3.5" />
              Avvia installer
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void installNow(false)}
            >
              Istruzioni manuali
            </Button>
          </div>
        </div>
      )}

      {info && <p className="text-[11px] text-muted-foreground">{info}</p>}
      {error && <p className="text-[11px] text-destructive">{error}</p>}
    </div>
  );
}
