# Risultati esperimento E2E

## Confronto tra configurazioni

| Config | Scenari ok | Step ok | Azioni/step media (mediana, p95) | Azioni non valide | Falsi successi | Falsi fallimenti | Tempo step mediana / p95 (ms) |
|---|---|---|---|---|---|---|---|
| q4_k_m | 0/20 | 0/35 | 15.00 (15.0, 15) | 0 (0.0%) | 1 | 0 | 23314 / 31928 |


## Config `q4_k_m`

Risorse: VRAM base 913 / picco 5971 / delta 5058 MiB; RAM picco llama-server 7853 MiB

| Scenario | Scenari ok | Step ok | Azioni/step media (mediana, p95) | Azioni non valide | Falsi successi | Falsi fallimenti | Tempo step mediana / p95 (ms) |
|---|---|---|---|---|---|---|---|
| ALL | 0/20 | 0/35 | 15.00 (15.0, 15) | 0 (0.0%) | 1 | 0 | 23314 / 31928 |
| S1 | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 22979 / 23011 |
| S1-it | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 1 | 0 | 22586 / 22678 |
| S2 | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 24576 / 32652 |
| S3 | 0/5 | 0/20 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 24101 / 31928 |

| Scenario | Prompt token media / max | Token generati media | Prefill tok/s mediana | Generazione tok/s mediana | Token snapshot mediana / p95 / max |
|---|---|---|---|---|---|
| ALL | 1561 / 2030 | 17 | 8528 | 144 | 672 / 740 / 745 |
| S1 | 1560 / 1825 | 14 | 8622 | 148 | 688 / 694 / 694 |
| S1-it | 1651 / 2030 | 26 | 8674 | 146 | 710 / 745 / 745 |
| S2 | 1564 / 1826 | 13 | 8390 | 142 | 671 / 689 / 689 |
| S3 | 1468 / 1747 | 13 | 8392 | 141 | 625 / 644 / 644 |

Motivi di chiusura dei passi: budget_exhausted: 19, done: 1, skipped: 15

Azioni non valide per tipo: -

Turni con piu' chiamate native: 0
