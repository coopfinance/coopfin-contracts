// Add this import at the top
use soroban_sdk::{contract, contractimpl, Env, Address, Symbol};

// Define the event
const ADMIN_TRANSFERRED: Symbol = Symbol::new("admin_transferred");

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    // Existing code...

    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();
        let stored_admin = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(stored_admin == current_admin, "Current admin does not match stored admin");
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish((ADMIN_TRANSFERRED, (current_admin, new_admin)));
    }
}

// Add this to your tests
#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, vec};

    #[test]
    fn test_transfer_admin() {
        let env = soroban_sdk::Env::default();
        let contract_id = env.register_contract(None, VotingContract);
        let client = VotingContractClient::new(&env, &contract_id);

        let admin = env.register_stellar_address("SA...");
        let new_admin = env.register_stellar_address("SB...");

        client.init(&admin);
        client.transfer_admin(&admin, new_admin.clone());

        let events = env.events().all();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, ADMIN_TRANSFERRED);
        assert_eq!(events[0].data, (admin, new_admin));
    }

    #[test]
    #[should_panic(expected = "Authorization required")]
    fn test_old_admin_cant_call_admin_only_functions() {
        let env = soroban_sdk::Env::default();
        let contract_id = env.register_contract(None, VotingContract);
        let client = VotingContractClient::new(&env, &contract_id);

        let admin = env.register_stellar_address("SA...");
        let new_admin = env.register_stellar_address("SB...");

        client.init(&admin);
        client.transfer_admin(&admin, new_admin.clone());

        // Attempt to call an admin-only function with the old admin
        client.create_proposal(&admin, "Proposal".to_string());
    }
}