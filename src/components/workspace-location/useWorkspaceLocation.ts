// Stato e azioni della scelta della posizione di un workspace (ambiente,
// cartella Windows, distro e percorso WSL), condivisi dai dialoghi "Nuovo
// workspace" e "Cambia cartella".

import { useCallback, useEffect, useState, type ClipboardEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { TargetKind } from "../../bindings/TargetKind";
import { api } from "../../lib/tauri";
import { chooseLocation, EMPTY_LOCATION, type LocationChoice, type LocationFields } from "../../lib/workspaceLocation";
import { matchDistro, slashes, toWslPath, wslBrowseStart } from "../../lib/wslPath";

const BROWSE_TITLE = "Seleziona cartella";

export interface WorkspaceLocation {
  fields: LocationFields;
  setKind: (kind: TargetKind) => void;
  setWinPath: (path: string) => void;
  selectDistro: (distro: string) => void;
  /** Percorso WSL digitato: solo slash, la conversione completa avviene all'uscita dal campo. */
  typeWslPath: (path: string) => void;
  /** Converte il percorso WSL scritto (anche `\\wsl.localhost\<distro>\...`) e seleziona la distro che nomina. */
  normalizeWslPath: () => void;
  pasteWslPath: (e: ClipboardEvent<HTMLInputElement>) => void;
  browse: () => Promise<void>;
  browseWsl: () => Promise<void>;
  verifyWsl: () => Promise<void>;
  distros: string[];
  loadingDistros: boolean;
  distroError: string | null;
  checking: boolean;
  /** Esito dell'ultima "Verifica" del percorso WSL (`null` = da verificare). */
  checkOk: boolean | null;
  /** La posizione da inviare, o l'errore da mostrare. */
  choose: () => LocationChoice;
  /** Riparte dai campi dati (es. la posizione attuale del workspace). */
  reset: (fields: LocationFields) => void;
}

/**
 * `isOpen`: il dialogo è aperto (le distro si caricano solo allora, in modalità WSL).
 * `onError`: errore di "Sfoglia", da mostrare nel dialogo.
 */
export function useWorkspaceLocation(isOpen: boolean, onError: (message: string) => void): WorkspaceLocation {
  const [fields, setFields] = useState<LocationFields>(EMPTY_LOCATION);
  const [distros, setDistros] = useState<string[]>([]);
  const [loadingDistros, setLoadingDistros] = useState(false);
  const [distroError, setDistroError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkOk, setCheckOk] = useState<boolean | null>(null);

  // Carica le distro quando si apre il dialogo in modalità WSL.
  useEffect(() => {
    if (!isOpen || fields.kind !== "wsl" || distros.length > 0 || loadingDistros) return;
    setLoadingDistros(true);
    setDistroError(null);
    api
      .listWslDistros()
      .then((list) => {
        setDistros(list);
        if (list.length === 1) setFields((f) => (f.distro ? f : { ...f, distro: list[0] }));
      })
      .catch((e) => setDistroError(String(e)))
      .finally(() => setLoadingDistros(false));
  }, [isOpen, fields.kind, distros.length, loadingDistros]);

  useEffect(() => {
    if (!isOpen) setCheckOk(null);
  }, [isOpen]);

  // Percorso WSL incollato, scelto con "Sfoglia" o lasciato nel campo: diventa Linux e, se nomina una
  // distro dell'elenco (`\\wsl.localhost\<distro>\...`), la seleziona. Restituisce i campi da usare subito.
  const applyWslPath = (raw: string): LocationFields => {
    const parsed = toWslPath(raw);
    const found = matchDistro(parsed.distro, distros);
    const next = { ...fields, wslPath: parsed.path, distro: found ?? fields.distro };
    if (next.wslPath !== fields.wslPath || next.distro !== fields.distro) setCheckOk(null);
    setFields(next);
    return next;
  };

  const pasteWslPath = (e: ClipboardEvent<HTMLInputElement>) => {
    const text = e.clipboardData.getData("text");
    if (!text) return;
    e.preventDefault();
    const input = e.currentTarget;
    const start = input.selectionStart ?? input.value.length;
    const end = input.selectionEnd ?? start;
    applyWslPath(input.value.slice(0, start) + text + input.value.slice(end));
  };

  const browse = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: BROWSE_TITLE });
      if (typeof selected === "string") setFields((f) => ({ ...f, winPath: selected }));
    } catch (e) {
      onError(String(e));
    }
  };

  const browseWsl = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: BROWSE_TITLE,
        defaultPath: fields.distro ? wslBrowseStart(fields.distro, fields.wslPath) : undefined,
      });
      if (typeof selected === "string") applyWslPath(selected);
    } catch (e) {
      onError(String(e));
    }
  };

  const verifyWsl = async () => {
    const target = applyWslPath(fields.wslPath);
    if (!target.distro || !target.wslPath) return;
    setChecking(true);
    setCheckOk(null);
    try {
      setCheckOk(await api.checkWslPath(target.distro, target.wslPath));
    } catch {
      setCheckOk(false);
    } finally {
      setChecking(false);
    }
  };

  const reset = useCallback((next: LocationFields) => {
    setFields(next);
    setCheckOk(null);
  }, []);

  return {
    fields,
    setKind: (kind) => setFields((f) => ({ ...f, kind })),
    setWinPath: (winPath) => setFields((f) => ({ ...f, winPath })),
    selectDistro: (distro) => {
      setFields((f) => ({ ...f, distro }));
      setCheckOk(null);
    },
    typeWslPath: (path) => {
      setFields((f) => ({ ...f, wslPath: slashes(path) }));
      setCheckOk(null);
    },
    normalizeWslPath: () => void applyWslPath(fields.wslPath),
    pasteWslPath,
    browse,
    browseWsl,
    verifyWsl,
    distros,
    loadingDistros,
    distroError,
    checking,
    checkOk,
    choose: () => chooseLocation(fields.kind === "wsl" ? applyWslPath(fields.wslPath) : fields),
    reset,
  };
}
