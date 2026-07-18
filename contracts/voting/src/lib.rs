use soroban_sdk::{contract, contractimpl, Env, Symbol, map, val};
use soroban_sdk::storage::{Instance, InstanceKey};

//...

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    //...

    pub fn vote(&self, env: Env, proposal_id: u32, choice: bool) {
        self.bump_instance(&env);

        // existing logic...

        // write vote
        let vote_map = map![&env, Symbol::new(&env, "votes")];
        vote_map.set(&env, &proposal_id, choice);

        // bump TTL for vote map
        env.storage().persistent().extend_ttl(&vote_map.key(), 1000);
    }

    //...

    fn bump_instance(&self, env: &Env) {
        env.storage().instance().extend_ttl(InstanceKey::from(env.contract.address()), 1000);
    }
}