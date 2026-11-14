// Percorsi dei workspace WSL scritti o incollati da Windows. Zero-dep, testato da tests/wsl-path.test.mjs.

/** Percorso Linux ricavato dall'input e, se l'input lo nomina (`\\wsl.localhost\<distro>\...`), la distro. */
export interface WslPathInput {
  path: string;
  distro: string | null;
}

const WSL_SHARE = /^\/\/(?:wsl\.localhost|wsl\$)\/([^/]+)(\/.*)?$/i;
const DRIVE = /^([a-z]):(\/.*)?$/i;

/** Solo i backslash diventano slash: va bene anche mentre si scrive, carattere per carattere. */
export function slashes(input: string): string {
  return input.replace(/\\/g, "/");
}

/**
 * Converte in percorso Linux quanto si incolla o si sceglie con "Sfoglia":
 * - `\\wsl.localhost\Ubuntu\home\me\x` e `\\wsl$\Ubuntu\home\me\x` → `/home/me/x`, distro `Ubuntu`;
 * - `C:\Users\me\x` → `/mnt/c/Users/me/x`;
 * - qualsiasi altro percorso: backslash → slash.
 * Toglie spazi e virgolette esterne ("Copia come percorso" di Esplora risorse le aggiunge), il prefisso
 * `\\?\` dei percorsi lunghi e lo slash finale. Un percorso già Linux resta com'è.
 */
export function toWslPath(input: string): WslPathInput {
  let path = slashes(
    input
      .trim()
      .replace(/^"(.*)"$/s, "$1")
      .trim(),
  );
  path = path.replace(/^\/\/\?\/UNC\//i, "//").replace(/^\/\/\?\//, "");

  let distro: string | null = null;
  const share = WSL_SHARE.exec(path);
  const drive = DRIVE.exec(path);
  if (share) {
    distro = share[1];
    path = share[2] ?? "/";
  } else if (drive) {
    path = `/mnt/${drive[1].toLowerCase()}${drive[2] ?? ""}`;
  }

  if (path.length > 1) path = path.replace(/\/+$/, "") || "/";
  return { path, distro };
}

/** La distro dell'elenco con quel nome, senza distinguere maiuscole e minuscole (Windows non le distingue). */
export function matchDistro(name: string | null, distros: readonly string[]): string | null {
  if (!name) return null;
  const lower = name.toLowerCase();
  return distros.find((d) => d.toLowerCase() === lower) ?? null;
}

/**
 * Cartella da cui aprire "Sfoglia" per una distro: quella del percorso già scritto, se è assoluto, altrimenti `/home`.
 * `\\wsl$` funziona anche sulle versioni di Windows 10 che non hanno `\\wsl.localhost`.
 */
export function wslBrowseStart(distro: string, currentPath: string): string {
  const { path } = toWslPath(currentPath);
  const linux = path.startsWith("/") && path !== "/" ? path : "/home";
  return `\\\\wsl$\\${distro}${linux.replace(/\//g, "\\")}`;
}
