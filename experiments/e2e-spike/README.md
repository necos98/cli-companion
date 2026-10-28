# e2e-spike

Esperimento isolato: LFM3-1.7B (GGUF, `llama-server` locale) pilota il browser tramite
agent-browser per eseguire test E2E scritti come passi in linguaggio naturale.
Risultati e conclusioni sono in [REPORT.md](REPORT.md).

Il crate e' indipendente dall'app (ha un `[workspace]` vuoto) e non tocca `src-tauri`.

## Requisiti

- Windows 10/11 x64 con GPU NVIDIA (build CUDA 12.4 di llama.cpp) e `nvidia-smi` nel PATH.
- Rust stable, Node.js con npm (agent-browser si installa in locale in questa cartella).
- Circa 4 GB liberi per i modelli e i binari, piu' Chrome for Testing (~200 MB, in `%USERPROFILE%\.agent-browser`).

## Comandi

Tutti dalla radice del repository:

```sh
# 1. Scarica e verifica (sha256) llama.cpp b11960 + cudart, i GGUF Q8_0 e Q4_K_M,
#    installa agent-browser 0.40.0 (npm locale) e Chrome for Testing. Idempotente.
cargo run --release --manifest-path experiments/e2e-spike/Cargo.toml -- setup

# 2. (Facoltativo) Il sito di prova, da esplorare a mano su http://127.0.0.1:8790
cargo run --release --manifest-path experiments/e2e-spike/Cargo.toml -- serve-fixture

# 3. Matrice completa: 5 configurazioni x 4 scenari x 5 esecuzioni
cargo run --release --manifest-path experiments/e2e-spike/Cargo.toml -- run

# Solo la configurazione base con S1, una esecuzione
cargo run --release --manifest-path experiments/e2e-spike/Cargo.toml -- run --configs base --scenarios S1 --runs 1
```

Opzioni di `run`: `--configs base,q4_k_m,thinking,native,delta`, `--scenarios S1,S1-it,S2,S3`,
`--runs N`, `--max-actions N` (default 15 per passo), `--llama-port` (default 18080),
`--fixture-port` (0 = effimera), `--out <cartella>`, `--headed`.

## Output

Ogni esecuzione scrive in `results/<data-ora>/` (oppure `--out`):

| File | Contenuto |
|---|---|
| `actions.jsonl` | un record per turno del modello: output grezzo, azione, esito, tempi, token, `timings` |
| `steps.jsonl` | un record per passo: verdetto, controlli falliti, motivo di fine, `done`, falsi successi, stato finale |
| `runs.jsonl` | un record per esecuzione di scenario |
| `summary.json`, `summary.md` | aggregati per configurazione e per scenario, VRAM e RAM |
| `meta.json` | versioni e sha256 dei componenti usati |
| `llama-<config>.log` | log di `llama-server` |

`*.jsonl` e `*.log` sono esclusi da git. In git c'e' solo il riepilogo della matrice usata dal report.

## Struttura

| Modulo | Ruolo |
|---|---|
| `setup/` | download con verifica sha256, estrazione, npm locale |
| `llama/server.rs` | processo `llama-server` (avvio, attesa di `/health`, arresto) |
| `llama/client.rs` | client HTTP compatibile OpenAI (`/v1/chat/completions`, `/tokenize`) |
| `browser/` | wrapper della CLI agent-browser (`--json`, ref scaduti, snapshot delta) |
| `agent/` | ciclo dell'agente: prompt, parsing delle azioni (modalita' `native` e `schema`), esecuzione |
| `scenario/` | scenari, dati di test, verdetto deterministico |
| `fixture/` | sito di prova servito dal crate (solo `127.0.0.1`), con `/__state`, `/__reset` e `/__seed` |
| `metrics/` | VRAM (`nvidia-smi`), RAM di picco, record JSONL e riepilogo |
| `experiment/` | matrice delle configurazioni e orchestrazione di `run` |
