#![no_std]

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

    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();
        Self::require_admin(&env, &current_admin);

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish(
            (Symbol::new(&env, "admin_transferred"),),
            (current_admin, new_admin),
        );
    }

    pub fn update_rules(env: Env, admin: Address, rules: CoopRules) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    fn setup() -> (Env, GovernanceContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let voting = Address::generate(&env);
        let loan = Address::generate(&env);
        let treasury = Address::generate(&env);
        client.initialize(&admin, &voting, &loan, &treasury);

        (env, client, admin)
    }

    fn replacement_rules() -> CoopRules {
        CoopRules {
            min_contribution: 20_0000000i128,
            contribution_period_days: 14,
            max_loan_multiplier: 2,
            loan_interest_bps: 300,
            voting_quorum: 2,
            voting_period_days: 5,
            late_penalty_bps: 100,
        }
    }

    #[test]
    fn test_transfer_admin_allows_new_admin_to_update_rules() {
        let (env, client, admin) = setup();
        let new_admin = Address::generate(&env);
        let rules = replacement_rules();

        client.transfer_admin(&admin, &new_admin);
        client.update_rules(&new_admin, &rules);

        let stored = client.get_rules();
        assert_eq!(stored.min_contribution, rules.min_contribution);
        assert_eq!(stored.voting_period_days, rules.voting_period_days);
    }

    #[test]
    #[should_panic]
    fn test_old_admin_cannot_call_admin_only_after_transfer() {
        let (env, client, admin) = setup();
        let new_admin = Address::generate(&env);
        client.transfer_admin(&admin, &new_admin);

        client.update_rules(&admin, &replacement_rules());
    }
}
