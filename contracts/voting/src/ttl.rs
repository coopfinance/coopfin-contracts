//! Storage TTL (time-to-live) helpers for the workspace.
//!
//! Soroban stores all contract state in either *instance* or *persistent*
//! storage, both of which expire if not refreshed. The protocol deletes
//! entries whose remaining TTL falls below the network's `MIN_TTL` and
//! refunds the rent it collected up front.
//!
//! To prevent silent loss of contributions, votes and proposals, every
//! state-changing function should:
//!
//! 1. Call [`bump_instance`] at the top to refresh contract-wide settings
//!    (admin, contract addresses, counters, configuration).
//! 2. Call [`bump_persistent`] immediately after any
//!    `.persistent().set(...)` to refresh that specific entry.
//!
//! # Why these values?
//!
//! Public Soroban network produces a new ledger roughly every 5 seconds,
//! so:
//!
//! - `THRESHOLD_LEDGERS = 100` gives us ~8 minutes of grace period before
//!   the network considers an entry cold.
//! - `EXTEND_TO_LEDGERS = 100_000` refreshes TTL to ~5.7 days, comfortably
//!   covering any realistic user-interaction gap without bloating rent.
//!
//! Both values are well within the SDK's `u32` range and match the
//! Soroban official examples for contracts with daily activity.
//!
//! # Activation
//!
//! As of the initial commit of this module, [`bump_instance`] and
//! [`bump_persistent`] are defined but not yet called from the
//! `#[contractimpl]` methods. A follow-up PR will wire them in once the
//! SDK version is pinned across the workspace and the exact
//! `extend_ttl` / `bump_ttl` API surface is confirmed.

use soroban_sdk::{Env, IntoVal, Symbol, Val};

/// Minimum remaining TTL (in ledgers) before storage is considered "cold".
pub const THRESHOLD_LEDGERS: u32 = 100;

/// TTL value (in ledgers) to extend storage to on each bump.
pub const EXTEND_TO_LEDGERS: u32 = 100_000;

/// Extend the TTL of this contract's instance storage.
///
/// Idempotent and cheap: if the contract has no instance storage entries
/// or they are already warm, this is a near no-op (~100 gas). Safe to
/// call from every public entry point.
pub fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(THRESHOLD_LEDGERS, EXTEND_TO_LEDGERS);
}

/// Extend the TTL of a single persistent-storage entry.
///
/// Call immediately after any `.persistent().set(key, value)`. Panics if
/// `key` was never written (matches Soroban's documented contract).
pub fn bump_persistent<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val> + Clone,
{
    env.storage()
        .persistent()
        .extend_ttl(key, THRESHOLD_LEDGERS, EXTEND_TO_LEDGERS);
}

/// Event tag emitted whenever a TTL bump is performed; useful for
/// off-chain indexers that want to verify storage hygiene without
/// subscribing to every state-changing event.
pub fn emit_bumped(env: &Env, target: &'static str) {
    let _ = env
        .events()
        .publish((Symbol::new(env, "ttl_bumped"), Symbol::new(env, target)), ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_within_range() {
        assert!(THRESHOLD_LEDGERS < EXTEND_TO_LEDGERS);
        assert!(THRESHOLD_LEDGERS >= 1);
        assert!(EXTEND_TO_LEDGERS <= u32::MAX);
    }

    #[test]
    fn extend_to_covers_minimum_one_week() {
        // 5 sec per ledger * EXTEND_TO_LEDGERS >= 7 days * 86400 sec
        // 100_000 * 5 = 500_000 sec > 604_800 sec
        assert!(EXTEND_TO_LEDGERS as u64 * 5 >= 7 * 86_400);
    }
}