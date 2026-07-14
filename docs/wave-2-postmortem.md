# Wave 2 Post-Mortem (running, update as tasks complete)

## Routing during wave 2
- MiniMax M3 = default
- Nemotron Ultra = research / doc tasks (task 2.9)
- GLM-5.2 = debugging only

## Dispatch strategy
Mostly sequential (post-wave-1 lesson). Round D (2.1, 2.4, 2.9) is the only fully-parallel round; tasks 2.1+2.4 are pure code with no shared files, task 2.9 is docs.

## Tasks

| # | Status | Commit | Notes |
|---|---|---|---|
| 2.1 stream processor | running | | 30 max-turns |
| 2.4 tool registry + read tool | running | | 30 max-turns |
| 2.9 Nemotron research | running | | 20 max-turns, docs only |
| 2.2 genai adapter | queued (round E) | | depends on 2.1 + 2.4 merge |
| 2.3 read tool | merged into 2.4 | n/a | brief updated |
| 2.5 AGENTS.md discovery | queued (round E) | | independent of 2.2/2.3 |
| 2.6 agent state machine | queued (round F) | | depends on 2.2 + 2.3 + 2.5 merge |
| 2.7 headless CLI | queued (round G) | | depends on 2.6 merge |
| 2.8 integration tests | queued (round G) | | depends on 2.6 + 2.7 merge |

## Things to watch

- **Wave 1 lesson:** implementers don't reliably have access to post-merge APIs of their dependencies. Solution: each task branches from main AFTER the previous task's merge. This is sequential, not parallel, except for round D.
- **Wave 1 lesson:** `pub mod` declarations get lost in auto-merges when multiple branches touch lib.rs. Solution: every merge runs `cargo test` to catch missing modules.
- **Wave 1 lesson:** max-turns 30 is the right budget for non-trivial work; max-turns 5 is a "fix one small thing" budget. 5-turn finishers often hang — finish manually instead.

## Failures
(append below as they happen)
