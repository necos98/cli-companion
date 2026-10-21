import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./tauri";
import type { AgentEnvStatus, SessionUsage, UsageLimitsReply, Workspace } from "./tauri";
import { CLAUDE_AGENT_ID } from "./session";
import type { AgentTabState } from "./agents";
import {
  LIMITS_POLL_MS,
  SESSION_POLL_MS,
  sessionPollInterval,
  shouldRefreshSessionUsage,
  workspaceEnvKey,
} from "./usage";

interface UsageTab {
  key: string;
  workspace: Workspace;
  agent?: AgentTabState;
}

/**
 * Polling dell'utilizzo di Claude Code: limiti per ambiente (all'avvio e ogni
 * 3 minuti) e token per sessione (a fine lavoro / richiesta di risposta /
 * avvio sessione e ogni 20 s mentre l'agente lavora). Con `enabled` falso non
 * parte alcuna chiamata e lo stato e' vuoto.
 */
/** Confronto per valore di risposte piccole (evita re-render con dati identici). */
const sameJson = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export function useUsage(enabled: boolean, agentEnvs: AgentEnvStatus[], tabs: UsageTab[]) {
  const [limits, setLimits] = useState<Record<string, UsageLimitsReply>>({});
  const [sessions, setSessions] = useState<Record<string, SessionUsage>>({});
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;

  // Ambienti con Claude Code installato (chiave stabile per l'effetto).
  const envKeys = agentEnvs
    .filter((e) => e.agent_id === CLAUDE_AGENT_ID && e.found && e.installed)
    .map((e) => e.env_key);
  const envSig = envKeys.join("\n");

  useEffect(() => {
    if (!enabled || envKeys.length === 0) {
      setLimits((p) => (Object.keys(p).length === 0 ? p : {}));
      return;
    }
    let cancelled = false;
    const load = () => {
      for (const env of envKeys) {
        api
          .agentUsageLimits(CLAUDE_AGENT_ID, env)
          .then((reply) => {
            if (cancelled) return;
            setLimits((p) => (env in p && sameJson(p[env], reply) ? p : { ...p, [env]: reply }));
          })
          .catch(() => undefined);
      }
    };
    load();
    const id = setInterval(load, LIMITS_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
    // envKeys e' derivato da envSig.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, envSig]);

  const refreshSession = useCallback((key: string, transcriptOverride?: string | null, agentOverride?: string | null) => {
    if (!enabledRef.current) return;
    const tab = tabsRef.current.find((t) => t.key === key);
    // Su session-start lo stato del tab non ha ancora id agente e transcript:
    // arrivano dall'evento.
    const agentId = agentOverride?.trim() || tab?.agent?.agentId;
    const transcript = transcriptOverride?.trim() || tab?.agent?.transcriptPath;
    if (!tab || !agentId || !transcript) return;
    api
      .agentSessionUsage(agentId, workspaceEnvKey(tab.workspace), transcript)
      .then((u) => {
        if (!enabledRef.current) return;
        setSessions((p) => {
          if (u === null) {
            if (!(key in p)) return p;
            const { [key]: _drop, ...rest } = p;
            return rest;
          }
          if (key in p && sameJson(p[key], u)) return p;
          return { ...p, [key]: u };
        });
      })
      .catch(() => undefined);
  }, []);

  /** Da chiamare a ogni evento dell'agente di un tab. */
  const onAgentEvent = useCallback(
    (key: string, event: string, transcriptPath?: string | null, agentId?: string | null) => {
      if (shouldRefreshSessionUsage(event)) refreshSession(key, transcriptPath, agentId);
    },
    [refreshSession],
  );

  useEffect(() => {
    if (!enabled) {
      setSessions((p) => (Object.keys(p).length === 0 ? p : {}));
      return;
    }
    // Alla riattivazione si rileggono i tab che hanno gia' un transcript.
    for (const t of tabsRef.current) refreshSession(t.key);
  }, [enabled, refreshSession]);

  // Ogni 20 s per i tab in cui l'agente sta lavorando: il timer esiste solo
  // finche' almeno un tab lavora.
  const anyWorking = tabs.some((t) => sessionPollInterval(t.agent?.phase) !== null);
  useEffect(() => {
    if (!enabled || !anyWorking) return;
    const id = setInterval(() => {
      for (const t of tabsRef.current) {
        if (sessionPollInterval(t.agent?.phase) !== null) refreshSession(t.key);
      }
    }, SESSION_POLL_MS);
    return () => clearInterval(id);
  }, [enabled, anyWorking, refreshSession]);

  // Tab chiusi: via i loro dati.
  const tabSig = tabs.map((t) => t.key).join("\n");
  useEffect(() => {
    const live = new Set(tabsRef.current.map((t) => t.key));
    setSessions((p) => {
      const keys = Object.keys(p);
      if (keys.every((k) => live.has(k))) return p;
      return Object.fromEntries(keys.filter((k) => live.has(k)).map((k) => [k, p[k]]));
    });
  }, [tabSig]);

  return { limits, sessions, onAgentEvent };
}
