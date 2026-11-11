# ConPTY distribuito con l'app

`conpty.dll` e `OpenConsole.exe` sono il motore console di Windows Terminal, redistribuibile. Il bundle li
installa accanto a `cli-companion.exe` (`bundle.resources` in `tauri.conf.json`) e `portable-pty` li carica al
posto del ConPTY di sistema: cerca `conpty.dll` nella cartella dell'eseguibile e, solo se manca, ripiega su
`kernel32.dll`.

## Perché

Il ConPTY incluso in Windows 10 (e nelle build meno recenti di Windows 11) non inoltra lo stream VT del
programma: lo ridisegna dal proprio buffer. Con le interfacce a schermo intero dei coding agent il risultato in
xterm.js è testo mescolato a residui di righe precedenti, cronologia che non si scorre fino in fondo e viste
(per esempio la trascrizione di Claude Code) che si sporcano. Il ConPTY recente non ha questi difetti, e
distribuirlo dà lo stesso comportamento su ogni versione di Windows.

## Provenienza

| | |
|---|---|
| Pacchetto | [`Microsoft.Windows.Console.ConPTY`](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY) |
| Versione | `1.25.260930003` |
| Architettura | x64 |
| Licenza | MIT, © Microsoft Corporation ([microsoft/terminal](https://github.com/microsoft/terminal)) |
| Requisito | Windows 10.0.17763 o successivo |

| File | Percorso nel pacchetto | SHA-256 |
|---|---|---|
| `conpty.dll` | `runtimes/win-x64/native/conpty.dll` | `feeef341d891643c62d30b6b07800bc70f0bc148f44cb8c3bee84aa557ae805a` |
| `OpenConsole.exe` | `build/native/runtimes/x64/OpenConsole.exe` | `3d66b23d0a71bb8eed2b77edc8b9df9bf54ce6c8fb74c863a30e760997f80586` |

Entrambi i file hanno la firma Authenticode di Microsoft Corporation
(`Get-AuthenticodeSignature conpty.dll, OpenConsole.exe`).

## Aggiornare

1. Scarica la nuova versione: `https://www.nuget.org/api/v2/package/Microsoft.Windows.Console.ConPTY/<versione>`
   (un `.nupkg` è uno zip).
2. Sostituisci i due file con quelli dei percorsi in tabella e verifica la firma.
3. Aggiorna versione e SHA-256 qui sopra e in `tests/conpty-bundle.test.mjs`.

I due file vanno aggiornati insieme: `conpty.dll` avvia l'`OpenConsole.exe` che trova nella propria cartella.
