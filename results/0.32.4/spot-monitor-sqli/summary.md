# waf-bench summary

## Overall (per group)

`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; `intercepted` = blocked + challenged.

| group | total | black | white | black blocked | black challenged | intercept rate | white blocked | white challenged | FP rate (blocked) | FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| spot-monitor-sqli | 61 | 31 | 30 | 10 | 0 | 32.3% | 0 | 0 | 0.00% | 0.00% | 6 | 19 | 0.00% | 0 | 0 |

## spot-monitor-sqli: black samples by category

| category | n | blocked | challenged | intercepted | +reject/error | intercept rate |
|---|---|---|---|---|---|---|
| other | 3 | 2 | 0 | 2 | 0 | 66.7% |
| rce | 3 | 0 | 0 | 0 | 0 | 0.0% |
| xss | 3 | 1 | 0 | 1 | 0 | 33.3% |
| deser | 3 | 0 | 0 | 0 | 0 | 0.0% |
| lfi | 3 | 2 | 0 | 2 | 0 | 66.7% |
| sqli | 3 | 0 | 0 | 0 | 0 | 0.0% |
| ssrf | 3 | 0 | 0 | 0 | 0 | 0.0% |
| ssti | 3 | 2 | 0 | 2 | 0 | 66.7% |
| crlf | 3 | 2 | 0 | 2 | 0 | 66.7% |
| xxe | 2 | 0 | 0 | 0 | 0 | 0.0% |
| log4shell | 1 | 1 | 0 | 1 | 0 | 100.0% |
| scanner | 1 | 0 | 0 | 0 | 0 | 0.0% |

## spot-monitor-sqli: white false positives by category

| category | n | blocked | challenged | FP rate (intercepted) | examples |
|---|---|---|---|---|---|

## spot-monitor-sqli: missed black samples (passed) by category

| category | missed | example files |
|---|---|---|
| other | 1 | `01/cd/82de0969852cbfec551a407988a5.black` |
| rce | 3 | `03/57/9d832d81d125f054d3e0593d944f.black`, `29/fc/c5a02830dd154507018058dad3af.black`, `2f/f0/1c77d9cb502c02a442ba36b0ad11.black` |
| xss | 2 | `00/e1/1e708ad1f095e8e21f57f4bb8cfc.black`, `05/ff/86504f3eb1098c3360626b095149.black` |
| deser | 3 | `06/da/18edf5c23081f8e2e679ce498858.black`, `17/9a/3ca003cd6941525e4293e84669f3.black`, `2d/c7/7c87e5745309e6615dc85506105e.black` |
| lfi | 1 | `29/2b/62b4f2b39173e65cc2cf8c76ad3c.black` |
| sqli | 3 | `1d/f1/80da3112e882e49bcb3b5d121fea.black`, `2c/74/4f7bf6d95f763dd1d844b9cdc6c3.black`, `33/7b/6c8e51054b7a83556df39f95c0ac.black` |
| ssrf | 3 | `4f/6e/13e7a46998deda814e5cdcec5473.black`, `38/57/251126732b39d45ef25e5778240c.black`, `59/51/176081cb991309741a15edec2639.black` |
| ssti | 1 | `5b/0d/13f7d5f4c37d750a1e23b19a9cb8.black` |
| crlf | 1 | `52/5a/c7c887b271e38260d483f503e2a6.black` |
| xxe | 2 | `89/67/48f3d90b343453994e53e942b2d9.black`, `c2/14/44c7ca191769289924c18167598b.black` |
| scanner | 1 | `5b/06/b33273adabbcb06926720e04cc1f.black` |
