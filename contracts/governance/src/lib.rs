#![no_std]
//! # CoopFin Governance Contract
//!
//! Central governance configuration for the cooperative. Stores
//! cooperative rules (minimum contribution, loan multiplier, interest
//! rates, voting parameters, etc.) and contract addresses for the other
//! modules. The admin can update rules; all members can read them.
//!
//! ## Rule Parameters
//!
//! - `min_contribution` — minimum contribution amount (in asset's smallest unit)
//! - `contribution_period_days` — expected interval between contributions
//! - `max_loan_multiplier` — maximum loan = multiplier × total contributions
//! - `loan_interest_bps` — annual interest rate in basis points
//! - `voting_quorum` — minimum number of votes required to pass a proposal
//! - `voting_period_days` — duration of the voting window
//! - `late_penalty_bps` — penalty rate for late contributions
//!
//! Default rules are set during initialization and represent sensible
//! starting parameters for an African ROSCA/SACCO.

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Symbol, Vec,
};

/// Storage keys for the governance contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address authorized to update rules.
    Admin,
    /// Address of the voting contract.
    VotingContract,
    /// Address of the loan contract.
    LoanContract,
    /// Address of the treasury contract.
    TreasuryContract,
    /// The current set of cooperative rules.
    Rules,
}

/// Cooperative rules configuration.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CoopRules {
    /// Minimum contribution in the asset's smallest unit (e.g. 10 USDC = 10_0000000).
    pub min_contribution: i128,
    /// Expected contribution interval in days.
    pub contribution_period_days: u32,
    /// Maximum loan = this multiplier × member's total contributions.
    pub max_loan_multiplier: u32,
    /// Annual loan interest rate in basis points (e.g. 500 = 5%).
    pub loan_interest_bps: u32,
    /// Minimum total votes required for a proposal to pass.
    pub voting_quorum: u32,
    /// Duration of the voting period in days.
    pub voting_period_days: u32,
    /// Late contribution penalty in basis points (e.g. 200 = 2%).
    pub late_penalty_bps: u32,
}

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract with default rules and
    /// contract addresses for all cooperative modules.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Panics
    ///
    /// No explicit panics — callable once per deploy.
    ///
    /// # Events
    ///
    /// No events emitted.
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
    /// Overwrites the stored [`CoopRules`] with new values. Only the
    /// admin may call this function.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()` and that `admin` matches the
    /// stored admin address.
    ///
    /// # Panics
    ///
    /// Panics if the caller is not the stored admin.
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
    /// # Authorization
    ///
    /// None — read-only query.
    ///
    /// # Panics
    ///
    /// Panics if the `Rules` storage key is missing (contract not
    /// initialized).
    ///
    /// # Returns
    ///
    /// The current [`CoopRules`] struct.
    pub fn get_rules(env: Env) -> CoopRules {
        env.storage().instance().get(&DataKey::Rules).unwrap()
    }

    /// Assert that `caller` is the stored admin, panicking otherwise.
    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}
