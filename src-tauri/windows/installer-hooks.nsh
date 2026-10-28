; Hook dell'installer NSIS di Tauri (bundle.windows.nsis.installerHooks).
;
; Dalla v0.9.2 l'identifier è "cli-companion" e Windows recapita i toast solo se
; il collegamento nel menu Start ha lo stesso AppUserModelID. In modalità
; aggiornamento (updater) il template NSIS non ricrea i collegamenti, quindi
; quelli esistenti conservano il vecchio ID e i toast vengono scartati in
; silenzio. Qui riapplichiamo l'ID ai soli collegamenti già presenti.

!macro RealignShortcutAppUserModelId SHORTCUT_PATH
  ${If} ${FileExists} "${SHORTCUT_PATH}"
    !insertmacro SetLnkAppUserModelId "${SHORTCUT_PATH}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro RealignShortcutAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro RealignShortcutAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
!macroend
