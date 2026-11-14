import { useEffect, useState, type ClipboardEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Loader2 } from "lucide-react";
import { Button } from "./ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { Input } from "./ui/input";
import { api } from "../lib/tauri";
import type { Workspace } from "../lib/tauri";
import { matchDistro, slashes, toWslPath, wslBrowseStart } from "../lib/wslPath";

interface Props {
  isOpen: boolean;
  onClose: () => void;
  onAdded: (ws: Workspace) => void;
}

export default function AddWorkspaceDialog({ isOpen, onClose, onAdded }: Props) {
  const [kind, setKind] = useState<"windows" | "wsl">("windows");
  const [name, setName] = useState("");
  const [winPath, setWinPath] = useState("");
  const [distro, setDistro] = useState("");
  const [wslPath, setWslPath] = useState("");
  const [shell, setShell] = useState("");
  const [distros, setDistros] = useState<string[]>([]);
  const [loadingDistros, setLoadingDistros] = useState(false);
  const [distroError, setDistroError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkOk, setCheckOk] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Carica le distro quando si apre il dialog in modalità WSL.
  useEffect(() => {
    if (!isOpen || kind !== "wsl" || distros.length > 0 || loadingDistros) return;
    setLoadingDistros(true);
    setDistroError(null);
    api
      .listWslDistros()
      .then((list) => {
        setDistros(list);
        if (list.length === 1) setDistro(list[0]);
      })
      .catch((e) => setDistroError(String(e)))
      .finally(() => setLoadingDistros(false));
  }, [isOpen, kind, distros.length, loadingDistros]);

  useEffect(() => {
    if (!isOpen) {
      setError(null);
      setCheckOk(null);
    }
  }, [isOpen]);

  const browse = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: "Seleziona cartella" });
      if (typeof selected === "string") setWinPath(selected);
    } catch (e) {
      setError(String(e));
    }
  };

  // Percorso WSL incollato, scelto con "Sfoglia" o lasciato nel campo: diventa Linux e, se nomina una
  // distro dell'elenco (`\\wsl.localhost\<distro>\...`), la seleziona. Restituisce i valori da usare subito.
  const applyWslPath = (raw: string) => {
    const parsed = toWslPath(raw);
    const found = matchDistro(parsed.distro, distros);
    if (found) setDistro(found);
    if (parsed.path !== wslPath || (found && found !== distro)) setCheckOk(null);
    setWslPath(parsed.path);
    return { path: parsed.path, distro: found ?? distro };
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

  const browseWsl = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Seleziona cartella",
        defaultPath: distro ? wslBrowseStart(distro, wslPath) : undefined,
      });
      if (typeof selected === "string") applyWslPath(selected);
    } catch (e) {
      setError(String(e));
    }
  };

  const verifyWsl = async () => {
    const target = applyWslPath(wslPath);
    if (!target.distro || !target.path) return;
    setChecking(true);
    setCheckOk(null);
    try {
      setCheckOk(await api.checkWslPath(target.distro, target.path));
    } catch {
      setCheckOk(false);
    } finally {
      setChecking(false);
    }
  };

  const save = async () => {
    setError(null);
    const wsl = kind === "wsl" ? applyWslPath(wslPath) : null;
    const path = wsl ? wsl.path : winPath.trim();
    if (!path) {
      setError(kind === "windows" ? "Seleziona una cartella." : "Inserisci il percorso Linux.");
      return;
    }
    if (wsl && !wsl.distro) {
      setError("Seleziona una distro WSL.");
      return;
    }
    setSaving(true);
    try {
      const ws = await api.addWorkspace({
        name: name.trim(),
        kind,
        path,
        distro: wsl ? wsl.distro : undefined,
        shell: shell.trim() || undefined,
      });
      onAdded(ws);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Nuovo workspace</DialogTitle>
        </DialogHeader>

        <DialogBody className="gap-3">
          <div className="flex flex-row gap-2">
            <Button
              size="sm"
              variant={kind === "windows" ? "default" : "outline"}
              onClick={() => setKind("windows")}
              className="flex-1"
            >
              Windows
            </Button>
            <Button
              size="sm"
              variant={kind === "wsl" ? "default" : "outline"}
              onClick={() => setKind("wsl")}
              className="flex-1"
            >
              WSL2
            </Button>
          </div>

          <div>
            <p className="mb-1 text-xs font-semibold">Nome (opzionale)</p>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Default: nome cartella" />
          </div>

          {kind === "windows" ? (
            <div>
              <p className="mb-1 text-xs font-semibold">Cartella</p>
              <div className="flex flex-row gap-2">
                <Input
                  value={winPath}
                  onChange={(e) => setWinPath(e.target.value)}
                  placeholder="C:\..."
                  className="flex-1"
                />
                <Button variant="secondary" onClick={() => void browse()}>
                  Sfoglia
                </Button>
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              <div>
                <p className="mb-1 text-xs font-semibold">Distro</p>
                {loadingDistros ? (
                  <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                ) : distroError ? (
                  <p className="text-xs text-destructive">{distroError}</p>
                ) : distros.length === 0 ? (
                  <p className="text-xs text-muted-foreground">Nessuna distro trovata. Installa WSL2 e riprova.</p>
                ) : (
                  <div className="flex flex-row flex-wrap gap-1.5">
                    {distros.map((d) => (
                      <Button
                        key={d}
                        size="sm"
                        variant={distro === d ? "default" : "outline"}
                        onClick={() => {
                          setDistro(d);
                          setCheckOk(null);
                        }}
                      >
                        {d}
                      </Button>
                    ))}
                  </div>
                )}
              </div>

              <div>
                <p className="mb-1 text-xs font-semibold">Percorso Linux</p>
                <div className="flex flex-row gap-2">
                  <Input
                    value={wslPath}
                    onChange={(e) => {
                      setWslPath(slashes(e.target.value));
                      setCheckOk(null);
                    }}
                    onPaste={pasteWslPath}
                    onBlur={() => applyWslPath(wslPath)}
                    placeholder="/home/user/progetto"
                    className="flex-1"
                  />
                  <Button variant="secondary" onClick={() => void browseWsl()}>
                    Sfoglia
                  </Button>
                  <Button
                    variant="secondary"
                    disabled={checking || !distro || !wslPath.trim()}
                    onClick={() => void verifyWsl()}
                  >
                    {checking ? "…" : "Verifica"}
                  </Button>
                </div>
                {checkOk === true && <p className="mt-1 text-xs text-status-finished">Cartella trovata.</p>}
                {checkOk === false && (
                  <p className="mt-1 text-xs text-destructive">Cartella non trovata nella distro.</p>
                )}
              </div>
            </div>
          )}

          <div>
            <p className="mb-1 text-xs font-semibold">Shell (opzionale)</p>
            <Input
              value={shell}
              onChange={(e) => setShell(e.target.value)}
              placeholder={kind === "windows" ? "powershell.exe" : "default della distro"}
            />
          </div>

          {error && <p className="text-xs text-destructive">{error}</p>}
        </DialogBody>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Annulla
          </Button>
          <Button disabled={saving} onClick={() => void save()}>
            {saving ? "Salvataggio…" : "Salva"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
