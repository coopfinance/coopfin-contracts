//! Governance contract for Soroban-based cooperative rule management.
//!
//! Stores and updates the cooperative's configurable rules (contribution
//! amounts, loan multipliers, voting quorum, etc.). Acts as the central
//! policy engine that the treasury, loan, and voting contracts defer to.
//!
//! The contract is `no_std` and Soroban-targeted.
//!
//! # Events
//!
//! - `rules_updated` — emitted when cooperative rules are changed.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol, Vec};

/// Storage keys for the governance contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address (set at initialization).
    Admin,
    /// Address of the voting contract.
    VotingContract,
    /// Address of the loan contract.
    LoanContract,
    /// Address of the treasury contract.
    TreasuryContract,
    /// The cooperative's configurable rules.
    Rules,
}

/// Cooperative rules governing contributions, loans, and voting.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CoopRules {
    /// Minimum contribution amount per period, in the asset's minor units.
    pub min_contribution: i128,
    /// Length of a contribution period in days.
    pub contribution_period_days: u32,
    /// Maximum loan multiplier (e.g. 3 = loan up to 3× total contributions).
    pub max_loan_multiplier: u32,
    /// Flat interest rate on loans, in basis points (e.g. 500 = 5%).
    pub loan_interest_bps: u32,
    /// Minimum number of votes required for a proposal to pass.
    pub voting_quorum: u32,
    /// Length of a voting period in days.
    pub voting_period_days: u32,
    /// Late payment penalty, in basis points (e.g. 200 = 2%).
    pub late_penalty_bps: u32,
}

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract with admin and dependent contract addresses.
    ///
    /// Sets default cooperative rules tuned for an African ROSCA/SACCO.
    ///
    /// # Authorization
    ///
    /// The `admin` must authenticate (via `require_auth`).
    ///
    /// # Events
    ///
    /// Emits no events directly; initialization is a one-time setup step.
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
            min_contribution: 10_0000000i128, // 10 USDC
            contribution_period_days: 30,
            max_loan_multiplier: 3,
            loan_interest_bps: 500, // 5%
            voting_quorum: 3,
            voting_period_days: 7,
            late_penalty_bps: 200, // 2% penalty
        };
        env.storage().instance().set(&DataKey::Rules, &rules);
    }

    /// Update the cooperative's configurable rules.
    ///
    /// # Authorization
    ///
    /// The `admin` must authenticate and be the current admin of the contract.
    ///
    /// # Panics
    ///
    /// Panics if `admin` is not the contract's admin.
    ///
    /// # Events
    ///
    /// Emits `rules_updated`.
    pub fn update_rules(env: Env, admin: Address, rules: CoopRules) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Rules, &rules);
        env.events().publish((Symbol::new(&env, "rules_updated"),), ());
    }

    /// Get the current cooperative rules.
    ///
    /// Read-only — no auth required.
    ///
    /// # Returns
    ///
    /// The current [`CoopRules`].
    pub fn get_rules(env: Env) -> CoopRules {
        env.storage().instance().get(&DataKey::Rules).unwrap()
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller {
            panic!("unauthorized");
        }
    }
}
