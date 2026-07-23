#![no_std]

//! Governance contract for cooperative rules and contract registry.
//!
//! Stores the [`CoopRules`] used by every other contract in the workspace
//! (loan interest, voting quorum, contribution period) plus the addresses of
//! the sibling contracts (`voting`, `loan`, `treasury`) that the coop needs
//! to talk to. Only the admin address registered at [`initialize`] can call
//! [`update_rules`].
//!
//! Defaults seeded in [`initialize`] are tuned for an African ROSCA / SACCO
//! (10 USDC minimum monthly contribution, 5% loan interest, 3-vote quorum).

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Symbol, Vec,
};

/// Storage keys used by [`GovernanceContract`].
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address authorized to mutate rules.
    Admin,
    /// Address of the [`VotingContract`] (set at initialize).
    VotingContract,
    /// Address of the [`LoanContract`] (set at initialize).
    LoanContract,
    /// Address of the [`TreasuryContract`] (set at initialize).
    TreasuryContract,
    /// Current [`CoopRules`] stored as an instance value.
    Rules,
}

/// Cooperative governance parameters shared across the workspace.
///
/// All numeric fields are intentionally simple integer types so they can be
/// persisted directly in Soroban instance storage and consumed by the loan,
/// voting, and treasury contracts.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CoopRules {
    /// Minimum monthly contribution in token base units (7 decimals on USDC).
    pub min_contribution: i128,
    /// Length of a contribution cycle in days.
    pub contribution_period_days: u32,
    /// Maximum loan size as a multiple of total contributions
    /// (e.g. `3` means a member can borrow up to 3x their contributed total).
    pub max_loan_multiplier: u32,
    /// Annualized loan interest rate in basis points (500 = 5.00%).
    pub loan_interest_bps: u32,
    /// Minimum number of yes votes required for a proposal to pass.
    pub voting_quorum: u32,
    /// Number of days a proposal stays open for voting.
    pub voting_period_days: u32,
    /// Penalty applied to late contributions, in basis points (200 = 2.00%).
    pub late_penalty_bps: u32,
}

/// Contract entry point. Single global instance per deployment.
#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract with admin and sibling-contract
    /// addresses, then seed sensible default [`CoopRules`].
    ///
    /// # Authorization
    /// Requires auth from `admin`; the same address becomes the only account
    /// that can mutate rules via [`update_rules`].
    ///
    /// # Events
    /// * topic `"governance_initialized"` — payload `(admin, voting, loan, treasury)`
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

        env.events().publish(
            (Symbol::new(&env, "governance_initialized"),),
            (admin, voting, loan, treasury),
        );
    }

    /// Replace the current [`CoopRules`] with the provided `rules`.
    ///
    /// # Panics
    /// Panics if `admin` is not the registered admin.
    ///
    /// # Events
    /// * topic `"rules_updated"` — payload `()`
    pub fn update_rules(env: Env, admin: Address, rules: CoopRules) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Rules, &rules);
        env.events().publish((Symbol::new(&env, "rules_updated"),), ());
    }

    /// Read the current [`CoopRules`].
    ///
    /// # Panics
    /// Panics if [`initialize`] has not yet been called.
    pub fn get_rules(env: Env) -> CoopRules {
        env.storage().instance().get(&DataKey::Rules).unwrap()
    }

    /// Internal: assert that `caller` is the admin registered at [`initialize`].
    ///
    /// # Panics
    /// Panics with `"unauthorized"` when `caller` does not match.
    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}