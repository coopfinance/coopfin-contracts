use soroban_sdk::{contract, contractimpl, Env, Symbol, Vec, map, val, panic_with_error};
use soroban_sdk::storage::{Instance, InstanceKey};

//...

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    //...

    pub fn contribute(&self, env: Env, amount: i128) {
        self.bump_instance(&env);

        // existing logic...

        // write contribution history
        let contribution_history = map![&env, Symbol::new(&env, "contribution_history")];
        contribution_history.set(&env, &env.ledger().sequence(), amount);

        // bump TTL for contribution history
        env.storage().persistent().extend_ttl(&contribution_history.key(), 1000);
    }

    //...

    fn bump_instance(&self, env: &Env) {
        env.storage().instance().extend_ttl(InstanceKey::from(env.contract.address()), 1000);
    }
}