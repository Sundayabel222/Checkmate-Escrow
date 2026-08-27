//! End-to-end tests: deploy the release WASM contracts and drive the complete
//! match lifecycle — create match → deposit ×2 → oracle records result →
//! submit result → payout — verifying state and balances at every step.

use e2e_tests::{deploy_and_fund, MINT_AMOUNT, STAKE, World};
use escrow::{
    errors::Error as EscrowError,
    types::{MatchState, Platform, Winner},
};
use oracle::{
    errors::Error as OracleError,
    types::MatchResult,
};
use soroban_sdk::{testutils::Address as _, Address, String};

/// Realistic Lichess game id (8 lowercase alphanumeric chars).
const GAME_ID: &str = "abcd1234";

/// Drives the full lifecycle for a given outcome and asserts every invariant
/// along the way.
fn assert_full_lifecycle(winner: Winner, result: MatchResult) {
    let World {
        env,
        escrow,
        oracle,
        token,
        token_client,
        oracle_admin,
        player1,
        player2,
    } = deploy_and_fund();

    // ── 1. Create match ──────────────────────────────────────────────────
    let id = escrow.create_match(
        &player1,
        &player2,
        &STAKE,
        &token,
        &String::from_str(&env, GAME_ID),
        &Platform::Lichess,
    );
    assert_eq!(id, 0, "first match should be assigned id 0");

    let pending = escrow.get_match(&id);
    assert_eq!(pending.state, MatchState::Pending, "match starts Pending");
    assert_eq!(escrow.get_escrow_balance(&id), 0, "nothing in escrow yet");

    // ── 2. Both players deposit ──────────────────────────────────────────
    escrow.deposit(&id, &player1);
    assert!(!escrow.is_funded(&id), "not funded after only player1 deposits");
    assert_eq!(escrow.get_escrow_balance(&id), STAKE);

    escrow.deposit(&id, &player2);
    assert!(escrow.is_funded(&id), "funded once both players deposit");
    assert_eq!(escrow.get_escrow_balance(&id), 2 * STAKE);
    assert_eq!(escrow.get_match(&id).state, MatchState::Active);

    // The escrow contract holds 2× stake; players are down their stakes.
    assert_eq!(token_client.balance(&escrow.address), 2 * STAKE);
    assert_eq!(token_client.balance(&player1), MINT_AMOUNT - STAKE);
    assert_eq!(token_client.balance(&player2), MINT_AMOUNT - STAKE);

    // ── 3. Oracle records the verified result ────────────────────────────
    oracle.submit_result(
        &id,
        &String::from_str(&env, GAME_ID),
        &result,
        &escrow.address,
    );
    assert!(oracle.has_result(&id), "oracle stores the result");
    let entry = oracle.get_result(&id);
    assert_eq!(entry.game_id, String::from_str(&env, GAME_ID));
    assert_eq!(entry.result, result);

    // ── 4. Oracle submits the result to escrow → payout ──────────────────
    escrow.submit_result(&id, &winner, &oracle_admin);
    assert_eq!(
        escrow.get_match(&id).state,
        MatchState::Completed,
        "match completes after payout"
    );
    assert_eq!(escrow.get_escrow_balance(&id), 0);
    assert_eq!(
        token_client.balance(&escrow.address),
        0,
        "contract must not retain funds after payout"
    );

    match winner {
        Winner::Player1 => {
            assert_eq!(token_client.balance(&player1), MINT_AMOUNT + STAKE);
            assert_eq!(token_client.balance(&player2), MINT_AMOUNT - STAKE);
        }
        Winner::Player2 => {
            assert_eq!(token_client.balance(&player1), MINT_AMOUNT - STAKE);
            assert_eq!(token_client.balance(&player2), MINT_AMOUNT + STAKE);
        }
        Winner::Draw => {
            assert_eq!(token_client.balance(&player1), MINT_AMOUNT);
            assert_eq!(token_client.balance(&player2), MINT_AMOUNT);
        }
    }

    // Note: event emission is asserted in the unit tests (contracts/escrow,
    // contracts/oracle). soroban-sdk 22 testutils does not expose events
    // published by WASM-registered contracts via env.events(), so the E2E
    // suite validates observable effects instead: state transitions, contract
    // and player balances, and stored oracle results.
}

#[test]
fn test_full_lifecycle_player1_wins() {
    assert_full_lifecycle(Winner::Player1, MatchResult::Player1Wins);
}

#[test]
fn test_full_lifecycle_player2_wins() {
    assert_full_lifecycle(Winner::Player2, MatchResult::Player2Wins);
}

#[test]
fn test_full_lifecycle_draw() {
    assert_full_lifecycle(Winner::Draw, MatchResult::Draw);
}

/// An impostor account must not be able to trigger a payout: the match stays
/// Active and the funds stay in escrow.
#[test]
fn test_non_oracle_cannot_submit_result() {
    let World {
        env,
        escrow,
        token,
        token_client,
        player1,
        player2,
        ..
    } = deploy_and_fund();

    let id = escrow.create_match(
        &player1,
        &player2,
        &STAKE,
        &token,
        &String::from_str(&env, "game_impostor"),
        &Platform::Lichess,
    );
    escrow.deposit(&id, &player1);
    escrow.deposit(&id, &player2);

    let impostor = Address::generate(&env);
    let result = escrow.try_submit_result(&id, &Winner::Player1, &impostor);
    assert_eq!(
        result,
        Err(Ok(EscrowError::Unauthorized)),
        "non-oracle submit_result must be rejected"
    );

    // Match untouched — still Active with both stakes still in escrow.
    assert_eq!(escrow.get_match(&id).state, MatchState::Active);
    assert_eq!(token_client.balance(&escrow.address), 2 * STAKE);
}

/// The oracle must refuse to record a result for a match_id that does not
/// exist in the escrow contract — no orphaned result entries.
#[test]
fn test_oracle_rejects_unknown_match() {
    let World { env, escrow, oracle, .. } = deploy_and_fund();

    let result = oracle.try_submit_result(
        &999u64,
        &String::from_str(&env, "ghost_game"),
        &MatchResult::Player1Wins,
        &escrow.address,
    );
    assert_eq!(
        result,
        Err(Ok(OracleError::MatchNotFound)),
        "oracle must reject results for unknown matches"
    );
    assert!(!oracle.has_result(&999u64), "nothing may be stored");
}
