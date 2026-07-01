#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Symbol,
};

// ─── TTL Constants ────────────────────────────────────────────────────────────
// Instance storage TTL: bump if below 100 ledgers (~5 days), extend to 10,000 (~500 days).
const INSTANCE_TTL_THRESHOLD: u32 = 100;
const INSTANCE_TTL_EXTEND_TO: u32 = 10_000;

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
    /// Extend instance storage TTL to prevent data expiration.
    ///
    /// Called at the start of every state-changing function. Uses threshold=100
    /// and extend_to=10,000 ledgers (~500 days) to keep instance data alive.
    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    pub fn initialize(
        env: Env,
        admin: Address,
        voting: Address,
        loan: Address,
        treasury: Address,
    ) {
        Self::bump_instance(&env);
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

    pub fn update_rules(env: Env, admin: Address, rules: CoopRules) {
        Self::bump_instance(&env);
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Rules, &rules);
        env.events().publish((Symbol::new(&env, "rules_updated"),), ());
    }

    pub fn get_rules(env: Env) -> CoopRules {
        env.storage().instance().get(&DataKey::Rules).unwrap()
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}
