import { useCallback, useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "./ui/button";
import { Badge } from "./ui/badge";
import { api } from "../lib/tauri";
import type { AgentEnvStatus } from "../lib/tauri";
import {
  agentEnvBadge,
  agentEnvState,
  agentEnvStateText,
  agentInstallLabel,
  agentRowKey,
  groupByAgent,
} from "../lib/agents";
import { cn } from "../lib/utils";

/** Tab "Coding agent": installa il plugin di notifica negli env (Windows + WSL). */
interface Props {
  /** Notifica l'elenco aggiornato degli ambienti (barra di stato). */
  onEnvs?: (envs: AgentEnvStatus[]) => void;
  /** Notifica il nuovo valore dell'interruttore "Mostra utilizzo". */
  onShowUsage?: (enabled: boolean) => void;
}

export default function AgentSettingsTab({ onEnvs, onShowUsage }: Props) {
  const [showUsage, setShowUsage] = useState(true);

  useEffect(() => {
    api
      .getLaunchSettings()
      .then((s) => setShowUsage(s.show_usage !== false))
      .catch(() => undefined);
  }, []);

  const toggleUsage = async (enabled: boolean) => {
    setShowUsage(enabled);
    try {
      const s = await api.setShowUsage(enabled);
      setShowUsage(s.show_usage !== false);
      onShowUsage?.(s.show_usage !== false);
    } catch {
      setShowUsage(!enabled);
    }
  };

  const [envs, setEnvs] = useState<AgentEnvStatus[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [log, setLog] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await api.agentEnvStatus();
      setEnvs(list);
      onEnvs?.(list);
      setLoadError(null);
    } catch (e) {
      setLoadError(String(e));
    }
  }, [onEnvs]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (env: AgentEnvStatus, action: "install" | "remove") => {
    setBusyKey(agentRowKey(env));
    setError(null);
    setLog(null);
    try {
      const out =
        action === "install"
          ? await api.installAgentPlugin(env.agent_id, env.env_key)
          : await api.removeAgentPlugin(env.agent_id, env.env_key);
      setLog(out);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyKey(null);
    }
  };

  const test = async () => {
    setTesting(true);
    setError(null);
    setLog(null);
    try {
      setLog(await api.testAgentNotify());
    } catch (e) {
      setError(String(e));
    } finally {
      setTesting(false);
    }
  };

  if (loadError) return <p className="text-xs text-destructive">{loadError}</p>;
  if (!envs)
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        Rilevamento ambienti…
      </div>
    );

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[11px] text-muted-foreground">
        Il plugin è globale: avvisa cli-companion quando un agente avviato in un
        suo terminale finisce o aspetta una risposta. Va installato in ogni
        ambiente usato, quindi in ogni distro WSL in cui lanci l'agente.
      </p>

      {groupByAgent(envs).map((group) => (
        <section key={group.agentId} className="flex flex-col gap-2">
          <h3 className="text-xs font-semibold">{group.agentName}</h3>
          {group.envs.map((env) => {
            const state = agentEnvState(env);
            const busy = busyKey === agentRowKey(env);
            const installLabel = agentInstallLabel(env);
            return (
              <div
                key={agentRowKey(env)}
                className="flex items-center gap-2 rounded-lg border bg-card p-2.5"
              >
                <Badge variant="secondary" className="font-mono">
                  {agentEnvBadge(env)}
                </Badge>
                <span
                  className={cn(
                    "h-2 w-2 flex-shrink-0 rounded-full",
                    state === "installed"
                      ? "bg-status-finished"
                      : state === "not-installed"
                        ? "bg-status-running"
                        : "bg-muted-foreground/40",
                  )}
                />
                <span className="min-w-0 flex-1 truncate text-xs">
                  {agentEnvStateText(env)}
                </span>
                {installLabel && (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() => void run(env, "install")}
                  >
                    {busy ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      installLabel
                    )}
                  </Button>
                )}
                {state === "installed" && (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => void run(env, "remove")}
                  >
                    Rimuovi
                  </Button>
                )}
              </div>
            );
          })}
        </section>
      ))}

      <div>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={showUsage}
            onChange={(e) => void toggleUsage(e.target.checked)}
            className="h-3.5 w-3.5 accent-primary"
          />
          Mostra utilizzo (limiti 5h/settimanali e token per sessione)
        </label>
        <p className="mt-1 text-[11px] text-muted-foreground">
          I limiti arrivano da un endpoint non documentato di Claude e potrebbero smettere di
          funzionare.
        </p>
      </div>

      <div>
        <Button
          size="sm"
          variant="outline"
          disabled={testing}
          onClick={() => void test()}
        >
          {testing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          Prova notifica
        </Button>
        <p className="mt-1 text-[11px] text-muted-foreground">
          Invia un segnale di prova attraverso lo stesso canale degli hook.
        </p>
      </div>

      {log && (
        <pre className="max-h-32 overflow-auto whitespace-pre-wrap rounded-lg border bg-muted/50 p-2.5 font-mono text-[11px]">
          {log}
        </pre>
      )}
      {error && (
        <p className="whitespace-pre-wrap text-xs text-destructive">{error}</p>
      )}
    </div>
  );
}
