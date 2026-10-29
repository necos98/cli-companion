import { useEffect, useRef } from "react";
import type { RefObject } from "react";

/**
 * Ctrl+Maiusc+O apre/chiude l'Ufficio. In fase di cattura, come Ctrl+Maiusc+B,
 * cosi' xterm non vede il tasto. Con un dialogo aperto non fa nulla; con
 * `enabled` falso (bot spenti) non registra niente. L'azione si legge da un
 * ref: il listener si registra una volta per cambio di `enabled`.
 */
export function useOfficeShortcut(
  enabled: boolean,
  modalOpenRef: RefObject<boolean>,
  toggle: () => void,
): void {
  const toggleRef = useRef(toggle);
  toggleRef.current = toggle;

  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.altKey || e.metaKey) return;
      if (e.key.toLowerCase() !== "o") return;
      if (modalOpenRef.current) return;
      e.preventDefault();
      e.stopPropagation();
      toggleRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [enabled, modalOpenRef]);
}
