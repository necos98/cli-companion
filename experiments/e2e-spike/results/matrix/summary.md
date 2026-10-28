# Risultati esperimento E2E

## Confronto tra configurazioni

| Config | Scenari ok | Step ok | Azioni/step media (mediana, p95) | Azioni non valide | Falsi successi | Falsi fallimenti | Tempo step mediana / p95 (ms) |
|---|---|---|---|---|---|---|---|
| base | 0/20 | 3/35 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 24699 / 26972 |
| q4_k_m | 0/6 | 0/6 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 22781 / 24058 |


## Config `base`

Risorse: VRAM base 1116 / picco 3544 / delta 2428 MiB; RAM picco llama-server 8265 MiB

| Scenario | Scenari ok | Step ok | Azioni/step media (mediana, p95) | Azioni non valide | Falsi successi | Falsi fallimenti | Tempo step mediana / p95 (ms) |
|---|---|---|---|---|---|---|---|
| ALL | 0/20 | 3/35 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 24699 / 26972 |
| S1 | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 24328 / 24701 |
| S1-it | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 25361 / 26218 |
| S2 | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 26642 / 26930 |
| S3 | 0/5 | 3/20 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 23472 / 27549 |

| Scenario | Prompt token media / max | Token generati media | Prefill tok/s mediana | Generazione tok/s mediana | Token snapshot mediana / p95 / max |
|---|---|---|---|---|---|
| ALL | 1515 / 1927 | 19 | 8758 | 102 | 653 / 690 / 690 |
| S1 | 1579 / 1912 | 20 | 8895 | 101 | 669 / 690 / 690 |
| S1-it | 1594 / 1917 | 21 | 8656 | 100 | 672 / 690 / 690 |
| S2 | 1599 / 1927 | 20 | 8975 | 102 | 666 / 689 / 689 |
| S3 | 1372 / 1746 | 16 | 8474 | 104 | 603 / 622 / 640 |

Motivi di chiusura dei passi: budget_exhausted: 23, skipped: 12

Azioni non valide per tipo: -

Turni con piu' chiamate native: 0

## Config `q4_k_m`

Risorse: VRAM base 907 / picco 2722 / delta 1815 MiB; RAM picco llama-server 3913 MiB

| Scenario | Scenari ok | Step ok | Azioni/step media (mediana, p95) | Azioni non valide | Falsi successi | Falsi fallimenti | Tempo step mediana / p95 (ms) |
|---|---|---|---|---|---|---|---|
| ALL | 0/6 | 0/6 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 22781 / 24058 |
| S1 | 0/5 | 0/5 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 22745 / 24058 |
| S1-it | 0/1 | 0/1 | 15.00 (15.0, 15) | 0 (0.0%) | 0 | 0 | 22850 / 22850 |

| Scenario | Prompt token media / max | Token generati media | Prefill tok/s mediana | Generazione tok/s mediana | Token snapshot mediana / p95 / max |
|---|---|---|---|---|---|
| ALL | 1560 / 1822 | 14 | 8667 | 150 | 688 / 694 / 694 |
| S1 | 1559 / 1822 | 14 | 8647 | 150 | 688 / 694 / 694 |
| S1-it | 1565 / 1821 | 14 | 8735 | 149 | 689 / 692 / 692 |

Motivi di chiusura dei passi: budget_exhausted: 6

Azioni non valide per tipo: -

Turni con piu' chiamate native: 0
