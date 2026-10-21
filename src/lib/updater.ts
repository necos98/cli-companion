// Updater: utility pure per il confronto versioni, in parità con
// `compare_versions` di src-tauri/src/updater.rs (usata dalla CLI --check-update).
// L'aggiornamento in-app passa da tauri-plugin-updater (UpdateCheck.tsx).
// File volutamente a zero dipendenze: importabile sia dall'app sia dai test
// `node --test` (type-stripping).

/** Normalizza: trim, via il prefisso `v`/`V`, stringa vuota -> "0". */
export function normalizeVersion(raw: string): string {
  const t = raw.trim().replace(/^[vV]/, "");
  return t === "" ? "0" : t;
}

function splitPart(part: string): [number, string] {
  const m = /^(\d*)(.*)$/.exec(part) ?? ["", "", ""];
  const num = m[1] === "" ? 0 : Number.parseInt(m[1], 10);
  return [Number.isSafeInteger(num) ? num : 0, m[2]];
}

/**
 * Confronta due versioni (stessa spec di `compare_versions` in Rust):
 * prefisso `v` ignorato, componenti mancanti = 0, parti numeriche come numeri,
 * release (suffisso vuoto) > prerelease. Ritorna -1 | 0 | 1.
 */
export function compareVersions(local: string, remote: string): -1 | 0 | 1 {
  const pa = normalizeVersion(local).split(".");
  const pb = normalizeVersion(remote).split(".");
  const n = Math.max(pa.length, pb.length);
  for (let i = 0; i < n; i++) {
    const [nx, rx] = splitPart(pa[i] ?? "0");
    const [ny, ry] = splitPart(pb[i] ?? "0");
    if (nx !== ny) return nx < ny ? -1 : 1;
    if (rx === ry) continue;
    if (rx === "") return 1; // release > prerelease
    if (ry === "") return -1;
    return rx < ry ? -1 : 1;
  }
  return 0;
}

/** `true` se `remote` è più recente di `local`. */
export function isNewerVersion(local: string, remote: string): boolean {
  return compareVersions(local, remote) === -1;
}
