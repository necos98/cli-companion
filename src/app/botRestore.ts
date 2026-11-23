import { api } from "../lib/tauri";
import { report } from "../lib/errors";
import { BOT_RESTORE_FAILED, botRestoreNotice, botRestoreTargets } from "../lib/botLaunch";
import type { TabEntry } from "../stores/tabs";

/**
 * Prima di aprire i tab ripristinati di un Bot, chiede al backend di rimettere
 * il loro ambiente (es. il modello di OpenCode): `pty_spawn` lo applica una
 * volta sola. Un tab che non si riesce a ripristinare per intero riparte
 * comunque; restituisce l'avviso per l'utente, `null` se è andato tutto bene.
 */
export async function stageBotRestoreEnvs(tabs: readonly TabEntry[]): Promise<string | null> {
  const warnings = await Promise.all(
    botRestoreTargets(tabs).map(async (target) => {
      try {
        const restored = await api.botRestoreEnv(target.workspaceId, target.botId, target.agentId, target.tabId);
        return restored.warning;
      } catch (e) {
        report("ripristino dell'ambiente di un Bot", e);
        return BOT_RESTORE_FAILED;
      }
    }),
  );
  return botRestoreNotice(warnings);
}

/**
 * Come `stageBotRestoreEnvs` per i tab ripristinati con un avvio integrato di
 * un agente: il backend rimette l'ambiente Headroom, se serve. Restituisce
 * l'avviso per l'utente, `null` se non c'è nulla da dire.
 */
export async function stageBuiltinRestoreEnvs(tabs: readonly TabEntry[]): Promise<string | null> {
  const warnings = await Promise.all(
    tabs
      .filter((t) => t.launchId?.startsWith("builtin:") && t.agentId !== undefined)
      .map(async (t) => {
        try {
          return await api.headroomStageEnv(t.workspaceId, t.agentId!, t.key);
        } catch (e) {
          report("ripristino dell'ambiente Headroom", e);
          return null;
        }
      }),
  );
  return botRestoreNotice(warnings);
}
