//! # Governance Contract
//!
//! Manages the cooperative's configurable rules and links together the
//! voting, loan, and treasury contracts. This contract serves as the
//! central registry for cooperative parameters such as minimum
//! contributions, interest rates, voting quorums, and penalty rules.
//!
//! ## Overview
//!
//! - The **admin** initializes the contract with references to the voting,
//!   loan, and treasury contracts, and sets default [`CoopRules`].
//! - The **admin** can update rules at any time via
//!   [`GovernanceContract::update_rules`].
//! - Any caller can read the current rules via
//!   [`GovernanceContract::get_rules`].
//!
//! Default rules are tuned for an African ROSCA/SACCO-style cooperative
//! with a 10 USDC minimum contribution, 30-day periods, 5% interest, and
//! a 3-vote quorum.
//!
//! ## Storage
//!
//! All configuration is stored in instance storage.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    VotingContract,
    LoanContract,
    TreasuryContract,
    Rules,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CoopRules {
    pub min_contribution: i128,
    pub contribution_period_days: u32,
    pub max_loan_multiplier: u32,   // e.g. 3 = max loan is 3x your total contributions
    pub loan_interest_bps: u32,
    pub voting_quorum: u32,
    pub voting_period_days: u32,
    pub late_penalty_bps: u32,
}

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract with linked contract addresses
    /// and default cooperative rules.
    ///
    /// Sets the admin, stores references to the voting, loan, and treasury
    /// contracts, and creates a default [`CoopRules`] configuration tuned
    /// for an African ROSCA/SACCO.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    ///
    /// None on first call (no duplicate-init guard).
    ///
    /// # Events
    ///
    /// None emitted directly by this function.
    ///
    /// # Return value
    ///
    /// Returns `()`.
    pub fn initialize(
        env: Env,
        admin: Address,
        voting: Address,
        loan: Address,
        treasury: Address,
    ) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::VotingContract, &voting);
        env.storage().instance().set(&DataKey::LoanContract, &loan);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);

        // Sensible defaults for an African ROSCA/SACCO
        let rules = CoopRules {
            min_contribution: 10_0000000i128,  // 10 USDC
            contribution_period_days: 30,
            max_loan_multiplier: 3,
            loan_interest_bps: 500,            // 5%
            voting_quorum: 3,
            voting_period_days: 7,
            late_penalty_bps: 200,             // 2% penalty
        };
        env.storage().instance().set(&DataKey::Rules, &rules);
    }

    /// Update the cooperative rules.
    ///
    /// Replaces the current [`CoopRules`] with a new set. It is the
    /// caller's responsibility (typically a governance vote) to ensure the
    /// new rules are valid and consistent.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`. Caller must also be the
    /// registered admin of this contract.
    ///
    /// # Panics
    ///
    /// - If the caller is not the admin (`"unauthorized"`).
    ///
    /// # Events
    ///
    /// - `"rules_updated"` — emitted with an empty tuple `()`.
    ///
    /// # Return value
    ///
    /// Returns `()`.
    pub fn update_rules(env: Env, admin: Address, rules: CoopRules) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Rules, &rules);
        env.events().publish((Symbol::new(&env, "rules_updated"),), ());
    }

    /// Get the current cooperative rules.
    ///
    /// Returns the active [`CoopRules`] configuration, including minimum
    /// contribution, period length, loan parameters, and voting settings.
    ///
    /// # Authorization
    ///
    /// None required — this is a read-only public query.
    ///
    /// # Panics
    ///
    /// - If the contract is not initialized (rules not set).
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Return value
    ///
    /// Returns [`CoopRules`] — the current cooperative rule set.
    pub fn get_rules(env: Env) -> CoopRules {
        env.storage().instance().get(&DataKey::Rules).unwrap()
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}
