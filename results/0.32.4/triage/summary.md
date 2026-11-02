# waf-bench summary

## Overall (per group)

`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; `intercepted` = blocked + challenged.

| group | total | black | white | black blocked | black challenged | intercept rate | white blocked | white challenged | FP rate (blocked) | FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| triage | 20 | 20 | 0 | 0 | 0 | 0.0% | 0 | 0 | 0.00% | 0.00% | 1 | 3 | 0.00% | 1 | 0 |

## triage: black samples by category

| category | n | blocked | challenged | intercepted | +reject/error | intercept rate |
|---|---|---|---|---|---|---|
| other | 19 | 0 | 0 | 0 | 0 | 0.0% |
| sqli | 1 | 0 | 0 | 0 | 0 | 0.0% |

## triage: white false positives by category

| category | n | blocked | challenged | FP rate (intercepted) | examples |
|---|---|---|---|---|---|

## triage: missed black samples (passed) by category

| category | missed | example files |
|---|---|---|
| other | 19 | `3e/3e/d7b61c3dfcd450e63e1cbeca4058.black`, `4b/0a/096825fe5d589a0fe75c44aca53e.black`, `56/e4/0a547696d93b1a50b4533c4bcfa2.black`, `6b/2a/83249e94cbaa8d93929c46c426bf.black`, `73/81/da87ccf03256c0a4f316afe35e04.black` |
| sqli | 1 |  |
