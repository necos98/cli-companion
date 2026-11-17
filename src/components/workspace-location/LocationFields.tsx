// I controlli della posizione di un workspace: la scelta dell'ambiente e i
// campi della cartella (Windows) o di distro e percorso (WSL). Lo stato è in
// `useWorkspaceLocation`.

import { Loader2 } from "lucide-react";
import type { TargetKind } from "../../bindings/TargetKind";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import type { WorkspaceLocation } from "./useWorkspaceLocation";

const KINDS: { kind: TargetKind; label: string }[] = [
  { kind: "windows", label: "Windows" },
  { kind: "wsl", label: "WSL2" },
];

/** Windows / WSL2. */
export function LocationKindToggle({ loc }: { loc: WorkspaceLocation }) {
  return (
    <div className="flex flex-row gap-2">
      {KINDS.map(({ kind, label }) => (
        <Button
          key={kind}
          size="sm"
          variant={loc.fields.kind === kind ? "default" : "outline"}
          onClick={() => loc.setKind(kind)}
          className="flex-1"
        >
          {label}
        </Button>
      ))}
    </div>
  );
}

/** La cartella dell'ambiente scelto. */
export function LocationPathFields({ loc }: { loc: WorkspaceLocation }) {
  return loc.fields.kind === "windows" ? <WindowsFolder loc={loc} /> : <WslFolder loc={loc} />;
}

function WindowsFolder({ loc }: { loc: WorkspaceLocation }) {
  return (
    <div>
      <p className="mb-1 text-xs font-semibold">Cartella</p>
      <div className="flex flex-row gap-2">
        <Input
          value={loc.fields.winPath}
          onChange={(e) => loc.setWinPath(e.target.value)}
          placeholder="C:\..."
          className="flex-1"
        />
        <Button variant="secondary" onClick={() => void loc.browse()}>
          Sfoglia
        </Button>
      </div>
    </div>
  );
}

function WslFolder({ loc }: { loc: WorkspaceLocation }) {
  const { fields, checking, checkOk } = loc;
  return (
    <div className="flex flex-col gap-3">
      <div>
        <p className="mb-1 text-xs font-semibold">Distro</p>
        <DistroPicker loc={loc} />
      </div>

      <div>
        <p className="mb-1 text-xs font-semibold">Percorso Linux</p>
        <div className="flex flex-row gap-2">
          <Input
            value={fields.wslPath}
            onChange={(e) => loc.typeWslPath(e.target.value)}
            onPaste={loc.pasteWslPath}
            onBlur={loc.normalizeWslPath}
            placeholder="/home/user/progetto"
            className="flex-1"
          />
          <Button variant="secondary" onClick={() => void loc.browseWsl()}>
            Sfoglia
          </Button>
          <Button
            variant="secondary"
            disabled={checking || !fields.distro || !fields.wslPath.trim()}
            onClick={() => void loc.verifyWsl()}
          >
            {checking ? "…" : "Verifica"}
          </Button>
        </div>
        {checkOk === true && <p className="mt-1 text-xs text-status-finished">Cartella trovata.</p>}
        {checkOk === false && <p className="mt-1 text-xs text-destructive">Cartella non trovata nella distro.</p>}
      </div>
    </div>
  );
}

function DistroPicker({ loc }: { loc: WorkspaceLocation }) {
  if (loc.loadingDistros) return <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />;
  if (loc.distroError) return <p className="text-xs text-destructive">{loc.distroError}</p>;
  if (loc.distros.length === 0) {
    return <p className="text-xs text-muted-foreground">Nessuna distro trovata. Installa WSL2 e riprova.</p>;
  }
  return (
    <div className="flex flex-row flex-wrap gap-1.5">
      {loc.distros.map((d) => (
        <Button
          key={d}
          size="sm"
          variant={loc.fields.distro === d ? "default" : "outline"}
          onClick={() => loc.selectDistro(d)}
        >
          {d}
        </Button>
      ))}
    </div>
  );
}
