# Report: LFM3-1.7B + llama-server + agent-browser per test E2E in linguaggio naturale

Data: 2026-10-28. Macchina: Windows 11, RTX 4060 8 GB (driver 617.42), Ryzen 5 5600X, 32 GB RAM.

## Esito in breve

**Raccomandazione: non integrare** LFM3-1.7B come esecutore di test E2E.

- Senza ragionamento (configurazioni `base` e `q4_k_m`) passano **0 scenari su 40**. Il modello reinvia
  il modulo in loop e non chiama mai `done`.
- Con il ragionamento acceso passano **9 scenari su 16** eseguiti. Un passo richiede pero' una mediana
  di 144 s (p95 268 s), e lo stesso test passa o fallisce da un'esecuzione all'altra.
- L'infrastruttura (gestione di `llama-server`, wrapper di agent-browser, verdetto da `/__state`,
  metriche) funziona ed e' riutilizzabile.

L'esperimento e' stato **interrotto su richiesta** dopo `base`, `q4_k_m` e parte di `thinking`.
Le configurazioni `native` e `delta` **non sono state eseguite** sulla matrice (vedi "Copertura").

## Versioni usate

| Componente | Versione |
|---|---|
| llama.cpp | release `b11960`, `version: 0.8.0-dev (build 11960, commit 8642541b2)` |
| Asset llama.cpp | `llama-b11960-bin-win-cuda-12.4-x64.zip` + `cudart-llama-bin-win-cuda-12.4-x64.zip` (sha256 dal digest GitHub) |
| Modello | `LiquidAI/LFM3-1.7B-GGUF` @ `b3f2ae5d73cd3c91bf40e8ba752c648aa05dffdf` |
| GGUF Q8_0 | `LFM3-1.7B-Q8_0.gguf`, sha256 `8f6b24182258205e9e01e21f1f24a1f208c1e8115578825d202ce39976af20b6` |
| GGUF Q4_K_M | `LFM3-1.7B-Q4_K_M.gguf`, sha256 `3793e1e6e2266919d6e015287221b41100175a98eddaba92e111aed3b05c93c4` |
| agent-browser | 0.40.0 (npm locale, binario nativo `agent-browser-win32-x64.exe`) |
| Chrome for Testing | 156.0.8078.73 |

Server: `llama-server --host 127.0.0.1 --jinja -c 32768 -fa on -ngl 99 -np 1`. Le assunzioni della
specifica sono confermate: tutti i layer sulla GPU via CUDA e contesto di 32K. Campionamento:
`temperature 0.1`, `top_k 50`, `repeat_penalty 1.1`, `seed = 1000 + esecuzione`. `max_tokens`
vale 512 col ragionamento spento e 4096 col ragionamento acceso. Il limite e' di 15 azioni per passo.

## Copertura

| Config | Fattore cambiato | Eseguita | Fonte |
|---|---|---|---|
| `base` | Q8_0, ragionamento spento, `schema`, snapshot completo | 20/20 esecuzioni | `results/matrix/` |
| `q4_k_m` | quantizzazione Q4_K_M | 20/20 | `results/matrix-rest/` |
| `thinking` | `enable_thinking=true` | 16/20 (S3 solo #1) | `results/matrix-rest/*.jsonl` (riepilogo calcolato dai JSONL) |
| `native` | chiamate a strumenti native | **no** | interrotto prima |
| `delta` | snapshot `--delta` | **no** | interrotto prima |

Note di onesta' sulla raccolta:

- La prima esecuzione della matrice (`results/matrix/`) si e' fermata durante `q4_k_m` (S1-it #2).
  La causa era un `net::ERR_ABORTED` transitorio di Chrome all'apertura della pagina iniziale,
  che l'harness trattava come fatale.
- Dopo la correzione (3 tentativi sull'apertura della pagina iniziale; le azioni del modello restano
  a un solo tentativo) le altre configurazioni sono ripartite da zero in `results/matrix-rest/`.
- Le 6 esecuzioni parziali di `q4_k_m` in `results/matrix/summary.md` sono scartate.
- `base` e' tenuta dalla prima esecuzione: il codice era identico salvo quel tentativo ripetuto,
  e nessuna esecuzione di `base` ne ha avuto bisogno.
- Nessuna esecuzione e' stata scelta a posteriori: tutte quelle completate sono nei conteggi.

## Risultati

Il successo di un passo lo decide lo stato vero (`/__state`, URL, testo della pagina), non `done()`.
Un passo fallito ferma lo scenario: i passi successivi risultano `skipped` e contano come non superati.

| Config | Scenari ok | Passi ok | Azioni/passo (media) | Azioni non valide | Falsi successi | Tempo passo mediana / p95 |
|---|---|---|---|---|---|---|
| `base` | **0/20** | 3/35 | 15.0 | 0 | 0 | 24.7 s / 27.0 s |
| `q4_k_m` | **0/20** | 0/35 | 15.0 | 0 | 1 | 23.3 s / 31.9 s |
| `thinking` | **9/16** | 12/19 | 12.2 | 16 (tutte `truncated`) | 1 | 143.8 s / 267.9 s |

Per scenario:

| Scenario | `base` | `q4_k_m` | `thinking` |
|---|---|---|---|
| S1 "go to /register and create an account" | 0/5 | 0/5 | 0/5 (account creato in 5/5, ma il passo non si chiude su `/welcome`) |
| S1-it "vai su /register e crea un account" | 0/5 | 0/5 | 3/5 (account creato in 4/5) |
| S2 email gia' registrata | 0/5 | 0/5 | 5/5 |
| S3 login -> settings -> nome -> verifica | 0/5 (passi 3/20) | 0/5 | 1/1 (4/4 passi) |

Con il ragionamento acceso l'account viene creato davvero in 9 delle 10 esecuzioni di S1/S1-it.
Pero' solo 3 si concludono sulla pagina attesa con `done`. Negli altri casi il modello prosegue
(va su `/login`, torna su `/register`) e il controllo predefinito `url_path_is(/welcome)` fallisce.
I controlli sono stati fissati prima delle esecuzioni e non sono stati cambiati.

### Latenza, throughput e risorse

| Metrica | `base` (Q8_0) | `q4_k_m` | `thinking` (Q8_0) |
|---|---|---|---|
| Token del prompt per chiamata (media / max) | 1515 / 1927 | ~1560 / 1822 | 1468 / 1891 |
| Token generati per chiamata (media) | 19 | 14 | 1101 (max 4096) |
| Prefill tok/s (mediana) | 8758 | ~8670 | 8848 |
| Generazione tok/s (mediana) | 102 | 150 | 104 |
| Snapshot in token (mediana / p95 / max) | 653 / 690 / 690 | ~688 / 694 / 694 | 653 / 709 / 710 |
| Tempo LLM per azione (mediana) | 333 ms | 264 ms | 6320 ms |
| Tempo browser per azione (mediana) | 1486 ms | 1487 ms | 251 ms |
| VRAM in uso: base -> picco (delta) | 1116 -> 3544 MiB (+2428) | 907 -> 2722 MiB (+1815)* | non registrata (esecuzione interrotta) |
| RAM di picco di `llama-server` | 8265 MiB** | 3913 MiB* | non registrata |

\* Per `q4_k_m` i valori di risorsa vengono dalla prima esecuzione, interrotta dopo 6 scenari.
Nella seconda `nvidia-smi` ha registrato un picco di 5971 MiB (+5058). `nvidia-smi` misura
la memoria dell'intera GPU, e un aumento di 3 GB a parita' di modello e contesto e' compatibile solo
con un altro processo attivo sulla GPU. Va letto come contaminato.

\*\* La RAM di picco non e' il modello: e' la cache dei prompt in RAM di llama.cpp (`--cache-ram`,
default 8192 MiB), che si riempie con le richieste. Va limitata (es. `--cache-ram 1024`).

Senza ragionamento il collo di bottiglia e' il browser. Ogni azione richiede circa 5 chiamate
alla CLI (azione, `wait --load networkidle`, URL, titolo, snapshot, testo), ognuna un processo
separato. Con il ragionamento domina la generazione: in media 1100 token a turno a circa 104 tok/s.

## Categorie di errore (esempi reali dai log)

1. **Invio prematuro in loop** (tutte le 10 esecuzioni S1/S1-it di `base`). Il modello compila uno o
   due campi e poi ripete "Create account" fino al limite di azioni. Non clicca mai la checkbox dei
   termini (`e59`), in nessuna esecuzione. Gli errori di validazione sono nel testo della pagina
   ("You must accept the terms."). Esempio, `base` S1 #1:
   `click e28 | fill e55="Dana Rivera" | click e60 | click e60 | fill e56=<email> | click e60` x 10.
2. **Non chiude mai il passo**. Su 43 passi eseguiti in `base` e `q4_k_m`, `done` viene chiamato una
   sola volta. In `base` S3 #1 i passi 1-3 sono superati (login, settings, nome cambiato:
   lo conferma `/__state`), ma sempre per esaurimento del limite. Il passo 4, una verifica che
   richiede `done(success=true)`, fallisce.
3. **Falso successo** (`q4_k_m` S1-it #4). Il modello dichiara `done(success=true)` con la nota
   "Registered account for Tomas Berg with email ... Account creation button (e60) clicked,
   submitting the form." Lo stato dice 0 account e l'URL e' ancora `/register`.
   E' lo stesso comportamento segnalato dal test indipendente citato nella specifica.
4. **Fa piu' di quanto chiesto**. In `base` S3 #1, al passo "open the settings page", il modello
   cambia anche il nome visualizzato. Al passo successivo clicca in sequenza `e172, e174, ..., e200`
   (Edit/Cancel che ridisegnano il DOM con riferimenti nuovi). In `thinking` S1 #4 crea l'account e
   poi naviga fino a `/app`: e' l'altro falso successo, perche' il controllo richiede `/welcome`.
5. **Ragionamento troncato** (`thinking`, 16 turni su 231). Il ragionamento esaurisce i 4096 token
   senza produrre un'azione (`finish_reason: length`). Esempio, S1 #3, coda del ragionamento:
   "So after clicking e60, the account creation likely completed? But we need to verify if there is
   a 'Create account' button still present..." Il modello rilegge lo snapshot invece di agire.
6. **Login inviato a meta'** (`base` S3 #2-#5). Il modello compila l'email e preme `Enter` prima di
   compilare la password, poi ripete il clic su "Sign in" (`click e57`, `click e140`) fino al limite.

Quando funziona, il ragionamento produce note corrette. Per esempio in `thinking` S2 #1:
"The /register page shows the error: 'This email is already registered.'" (5/5 in S2, nessun
account creato).

## Verifiche pratiche

| Verifica | Esito |
|---|---|
| agent-browser su Windows nativo | **Funziona.** Il pacchetto npm 0.40.0 include `agent-browser-win32-x64.exe`; `agent-browser install` scarica Chrome for Testing in `%USERPROFILE%\.agent-browser`. Note: gli snapshot `-i` non contengono testo statico (messaggi di errore e `role=alert` compresi), quindi l'harness aggiunge `get text body`. Dopo una navigazione i riferimenti cambiano (`Unknown ref: eN`). `--delta` risponde `full`, `delta` o `unchanged`. |
| Chiamate a strumenti di LFM3 in llama.cpp | **Funziona.** Con `--jinja` il formato XML di LFM3 viene riconosciuto e restituito come `tool_calls` OpenAI (`finish_reason: tool_calls`), col ragionamento sia spento sia acceso. Anche `response_format` con `json_schema` (`oneOf` + `const`) funziona. Col ragionamento acceso, in modalita' `schema` il modello non vede lo schema: va descritto nel prompt. |
| Sito servito in WSL raggiungibile da Windows | **Funziona.** `python3 -m http.server 8765 --bind 127.0.0.1` dentro Ubuntu-24.04 (WSL2, NAT, senza `.wslconfig`) risponde su `http://localhost:8765/` sia a `curl` sia ad agent-browser. |

Altre note:

- **Arresto di `llama-server` su Windows.** `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT)` non ha effetto
  sul processo lanciato con `CREATE_NO_WINDOW`. L'unico arresto affidabile e' terminare il processo
  (circa 290 ms); porta e VRAM vengono liberate.
- **Cache dei prompt.** `cache_n` ha una mediana di 399 token, cioe' viene riutilizzato solo il prefisso
  fisso (il prompt di sistema). In modalita' snapshot completo l'osservazione precedente viene
  riscritta a ogni turno, e questo invalida il resto del prefisso. Con circa 8800 tok/s di prefill
  il costo e' trascurabile.

## Configurazione consigliata (se si riprendesse l'esperimento)

Fra quelle misurate solo `thinking` (Q8_0, ragionamento acceso, `schema`, snapshot completo) produce
successi reali. Va aggiunto un limite al ragionamento (`--reasoning-budget`, oppure un `max_tokens`
piu' alto per evitare i troncamenti) e `--cache-ram 1024`. Anche cosi' non e' adatta a un uso
quotidiano: costa da 1 a 4 minuti per passo e il 44% degli scenari fallisce.

## Raccomandazione

**Non integrare** LFM3-1.7B come esecutore di test E2E in cli-companion.

- Senza ragionamento il modello non porta a termine nemmeno una registrazione (0/40).
- Con il ragionamento passa poco piu' di meta' degli scenari, con tempi da 10 a 100 volte quelli di
  un test scritto a mano.
- Lo stesso scenario passa o fallisce da un'esecuzione all'altra (S1-it: 3/5). Per un sistema di test
  questo e' il difetto peggiore: un fallimento non dice piu' se l'app e' rotta.
- I falsi successi ci sono (2 sui passi eseguiti) e il modello non sa fermarsi, quindi `done()` non e'
  affidabile come segnale.

Cosa resta valido per una seconda iterazione:

- Lo stesso harness con un modello piu' capace dietro la stessa API compatibile OpenAI: il client
  non dipende dal modello.
- Il modello come *traduttore*: converte una volta i passi in linguaggio naturale in uno script
  deterministico (azioni + selettori), verificato e poi rieseguito senza LLM.
- In ogni caso, il verdetto deve venire dallo stato, mai dal modello.

## Cosa si potra' spostare in `src-tauri`

Il codice e' sincrono (thread std, `Result<_, String>`), senza tokio, nello stile di
`src-tauri/src/agent_notify.rs`.

| Modulo | Spostabile | Note |
|---|---|---|
| `llama/server.rs` | si', cosi' com'e' | E' il futuro pulsante "Avvia": avvio su `127.0.0.1`, attesa di `/health`, arresto, RAM di picco. Aggiungere `--cache-ram` alla configurazione. |
| `llama/client.rs` | si' | Client generico `/v1/chat/completions` + `/tokenize`, con `timings`. |
| `browser/` | si' | Wrapper della CLI con `--json`, ref scaduti con un nuovo tentativo, tracker dei delta. |
| `agent/` | si', se si riprende l'idea | Ciclo, parsing delle azioni nelle due modalita', prompt. |
| `setup/` | in parte | Download con sha256 ed estrazione zip (`zip` e `sha2` hanno le stesse versioni di `src-tauri`). L'app dovrebbe usare il proprio aggiornatore e i propri percorsi. |
| `metrics/gpu.rs`, `process_mem.rs` | si' | `nvidia-smi` e `K32GetProcessMemoryInfo` (`windows-sys` 0.59, gia' dipendenza dell'app). |
| `fixture/`, `scenario/`, `experiment/` | no | Specifici dell'esperimento. |

Dipendenza nuova rispetto all'app: `ureq` 3 (client HTTP sincrono). In `src-tauri` si puo' tenere
oppure sostituire con `reqwest` bloccante, gia' presente in modo transitivo.
