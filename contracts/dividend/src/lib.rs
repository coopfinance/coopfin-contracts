#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String, Symbol, Vec,
};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    AssetAddress,
    TreasuryContract,
    Distributions,
    DistributionCounter,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Distribution {
    pub id: u32,
    pub total_profit: i128,
    pub total_shares: i128,
    pub recipients: Vec<Address>,
    pub amounts: Vec<i128>,
    pub executed_at: u64,
    pub period: String,
}

#[contract]
pub struct DividendContract;

#[contractimpl]
impl DividendContract {
    pub fn initialize(env: Env, admin: Address, asset: Address, treasury: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::AssetAddress, &asset);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::DistributionCounter, &0u32);
        env.storage().instance()
            .set(&DataKey::Distributions, &Vec::<Distribution>::new(&env));
    }

    /// Transfer admin rights to a new address.
    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();
        Self::require_admin(&env, &current_admin);

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish(
            (Symbol::new(&env, "admin_transferred"),),
            (current_admin, new_admin),
        );
    }

    /// Distribute profit proportionally based on each member's share weight.
    ///
    /// `recipients` and `shares` must be equal length.
    /// Each member receives: `profit * (member_shares / total_shares)`
    pub fn distribute(
        env: Env,
        admin: Address,
        recipients: Vec<Address>,
        shares: Vec<i128>,
        total_profit: i128,
        period: soroban_sdk::String,
    ) -> u32 {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        if recipients.len() != shares.len() {
            panic!("recipients and shares length mismatch");
        }
        if total_profit <= 0 { panic!("profit must be positive"); }

        let total_shares: i128 = shares.iter().sum();
        if total_shares == 0 { panic!("total shares cannot be zero"); }

        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);

        let mut amounts: Vec<i128> = Vec::new(&env);

        for i in 0..recipients.len() {
            let member_shares = shares.get(i).unwrap();
            let payout = (total_profit * member_shares) / total_shares;
            if payout > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &recipients.get(i).unwrap(),
                    &payout,
                );
            }
            amounts.push_back(payout);
        }

        let counter: u32 = env.storage().instance()
            .get(&DataKey::DistributionCounter).unwrap_or(0);
        let id = counter + 1;

        let dist = Distribution {
            id,
            total_profit,
            total_shares,
            recipients: recipients.clone(),
            amounts: amounts.clone(),
            executed_at: env.ledger().timestamp(),
            period,
        };

        let mut distributions: Vec<Distribution> = env.storage().instance()
            .get(&DataKey::Distributions).unwrap_or(Vec::new(&env));
        distributions.push_back(dist);
        env.storage().instance().set(&DataKey::Distributions, &distributions);
        env.storage().instance().set(&DataKey::DistributionCounter, &id);

        env.events().publish(
            (Symbol::new(&env, "dividend_distributed"),),
            (id, total_profit, recipients.len()),
        );
        id
    }

    pub fn get_distributions(env: Env) -> Vec<Distribution> {
        env.storage().instance()
            .get(&DataKey::Distributions)
            .unwrap_or(Vec::new(&env))
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
    use soroban_sdk::{token::StellarAssetClient, Env, String, Vec};

    fn setup() -> (Env, DividendContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, DividendContract);
        let client = DividendContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let asset = env.register_stellar_asset_contract_v2(token_admin);
        let asset_address = asset.address();

        StellarAssetClient::new(&env, &asset_address).mint(&contract_id, &1_000_0000000i128);
        client.initialize(&admin, &asset_address, &treasury);

        (env, client, admin)
    }

    fn single_recipient_distribution(env: &Env) -> (Vec<Address>, Vec<i128>) {
        let recipient = Address::generate(env);
        let mut recipients = Vec::new(env);
        let mut shares = Vec::new(env);
        recipients.push_back(recipient);
        shares.push_back(1i128);
        (recipients, shares)
    }

    #[test]
    fn test_transfer_admin_allows_new_admin_to_distribute() {
        let (env, client, admin) = setup();
        let new_admin = Address::generate(&env);
        let (recipients, shares) = single_recipient_distribution(&env);

        client.transfer_admin(&admin, &new_admin);
        let distribution_id = client.distribute(
            &new_admin,
            &recipients,
            &shares,
            &100_0000000i128,
            &String::from_str(&env, "2026-07"),
        );

        assert_eq!(distribution_id, 1);
        assert_eq!(client.get_distributions().len(), 1);
    }

    #[test]
    #[should_panic]
    fn test_old_admin_cannot_call_admin_only_after_transfer() {
        let (env, client, admin) = setup();
        let new_admin = Address::generate(&env);
        let (recipients, shares) = single_recipient_distribution(&env);
        client.transfer_admin(&admin, &new_admin);

        client.distribute(
            &admin,
            &recipients,
            &shares,
            &100_0000000i128,
            &String::from_str(&env, "2026-07"),
        );
    }
}
