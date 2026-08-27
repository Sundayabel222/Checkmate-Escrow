# E2E Testing

This document describes the end-to-end (E2E) test suite in `e2e-tests/` and
what it validates before a release.

## Why E2E tests

The unit tests in `contracts/escrow` and `contracts/oracle` register the
contracts natively (compiled as part of the test binary). They are fast and
cover the state machine well, but they do not prove that the **deployed
artifact** — the release WASM that would actually be shipped to a network —
behaves correctly, nor that the two contracts work together.

The E2E suite closes that gap:

1. It builds the release WASM artifacts (`target/wasm32v1-none/release/*.wasm`),
2. it deploys them into the sandboxed Soroban host, and
3. it drives the complete match lifecycle exactly as a user and the off-chain
   oracle service would.

## What the E2E suite covers

### The gold-standard lifecycle

`e2e-tests/tests/lifecycle.rs` runs the full flow end to end for every payout
branch:

1. **Deploy** — `escrow.wasm` and `oracle.wasm` (the release bytecode) are
   registered in the sandbox, together with a Stellar asset contract that
   plays the role of the stake token.
2. **Initialize** — escrow is configured with the oracle service account as
   its trusted oracle; the oracle contract gets the same account as admin.
3. **Create match** — `create_match` with a realistic Lichess `game_id`
   (`"abcd1234"`). The match is `Pending` and escrow holds nothing.
4. **Deposit** — player1 then player2 transfer their stakes in. The match
   activates, `is_funded` flips to `true`, and the contract balance is exactly
   `2 × stake`.
5. **Oracle records the result** — the oracle service calls
   `oracle.submit_result`, which verifies the match exists in escrow via a
   cross-contract call, then stores the `ResultEntry`.
6. **Submit result → payout** — the oracle calls `escrow.submit_result`. The
   winner receives `2 × stake`, the loser receives nothing (or both players
   receive their stake back on a draw), the match is `Completed`, and the
   contract retains a zero balance.

Assertions cover state transitions, the contract's token balance at each
stage, player balances after payout, and stored oracle results.

> **Note on events:** event emission (`match/created`, `match/activated`,
> `match/completed`, `oracle/result`) is asserted in the unit tests in
> `contracts/*`. soroban-sdk 22's test host does not expose events published
> by *WASM-registered* contracts through `env.events()` (only diagnostics are
> recorded), so the E2E suite validates observable effects instead — the
> state and balance assertions above pin the same behavior end to end.

### Payout branches

| Test | Outcome | Verifies |
|------|---------|----------|
| `test_full_lifecycle_player1_wins` | `Winner::Player1` | winner ends with `initial + stake` |
| `test_full_lifecycle_player2_wins` | `Winner::Player2` | winner ends with `initial + stake` |
| `test_full_lifecycle_draw` | `Winner::Draw` | each player gets exactly their stake back |

### Negative paths

- `test_non_oracle_cannot_submit_result` — an impostor account cannot trigger
  a payout; the match stays `Active` and both stakes stay in escrow.
- `test_oracle_rejects_unknown_match` — the oracle refuses to record a result
  for a `match_id` that does not exist in escrow (no orphaned result entries).

## How to run

Contracts are built for **`wasm32v1-none`** — the Soroban target that emits
core wasm 1.0 only (see `scripts/build.sh`). Do not build with
`wasm32-unknown-unknown`: on Rust 1.82+ that target enables
reference-types / multi-value instructions, which the Soroban host validator
rejects at deployment (`reference-types not enabled`) — the E2E suite would
catch exactly this.

```bash
# Full suite: build WASM → unit tests → E2E tests
scripts/test.sh

# Or manually:
cargo build --target wasm32v1-none --release
cargo test              # unit tests
cargo test -p e2e-tests # E2E tests against the release WASM
```

The E2E tests read the compiled artifacts from `target/wasm32v1-none/release/`
at runtime, so the WASM build must run first. If the artifacts are missing,
the tests fail with a clear message instead of silently testing stale
bytecode.

Note: `e2e-tests` is a workspace member but not a *default* member. Plain
`cargo build` / `cargo test` compile only the contracts — the harness is
host-only (it reads WASM from disk) and would not compile for the wasm
target. Run it explicitly with `cargo test -p e2e-tests`.

## CI

`.github/workflows/ci.yml` builds the release WASM in the `test` job and then
runs both `cargo test` and `cargo test -p e2e-tests`, so every push to `main`
and every PR is validated against the deployable artifacts. The `build` job
uploads the same WASM artifacts as release candidates.

## Coverage gaps (future work)

The suite is fully automated but runs in an in-process sandbox. It does not
cover:

- **Live network behavior** — a standalone `stellar-core` + Soroban RPC node
  (e.g. `stellar/quickstart` in Docker) driven via `stellar-cli` or the RPC
  would validate fees, sequencing, and real ledger effects. Recommended as a
  pre-mainnet-release run, not as part of every PR.
- **Off-chain oracle HTTP fetch** — the oracle service's Lichess / Chess.com
  API calls are out of scope; the E2E mocks the service side. See
  [docs/oracle.md](oracle.md) for the game-id formats the service validates.
- **TTL / expiry across real ledgers** — `expire_match` timing is only
  partially simulated in the unit tests.
- **Multi-match concurrency and stress**.
