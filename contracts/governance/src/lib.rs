#![no_std]
//! Cooperative rule configuration contract for CoopFinance.
//!
//! Stores contract addresses and mutable cooperative rules such as contribution
//! cadence, loan limits, interest, voting quorum, and late penalties.

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Symbol, Vec,
};

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
    /// Initializes linked contract addresses and default cooperative rules.
    ///
    /// # Authorization
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    /// Does not explicitly guard against reinitialization.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns nothing.
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

    /// Replaces the active cooperative rules.
    ///
    /// # Authorization
    /// Requires authorization from `admin`, which must match the stored admin.
    ///
    /// # Panics
    /// Panics if `admin` is not the stored admin.
    ///
    /// # Events
    /// Emits `rules_updated`.
    ///
    /// # Returns
    /// Returns nothing.
    pub fn update_rules(env: Env, admin: Address, rules: CoopRules) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Rules, &rules);
        env.events().publish((Symbol::new(&env, "rules_updated"),), ());
    }

    /// Returns the current cooperative rules.
    ///
    /// # Authorization
    /// No authorization is required.
    ///
    /// # Panics
    /// Panics if rules have not been initialized.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns the stored `CoopRules`.
    pub fn get_rules(env: Env) -> CoopRules {
        env.storage().instance().get(&DataKey::Rules).unwrap()
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}
