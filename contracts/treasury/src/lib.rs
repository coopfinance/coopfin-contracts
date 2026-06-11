#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Map, Symbol, Vec, String,
};

/// ─── Storage Keys ────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    GroupName,
    Members,
    Contributions(Address),
    TotalContributions,
    AssetAddress,
    IsActive,
}

/// ─── Types ───────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ContributionRecord {
    pub member: Address,
    pub amount: i128,
    pub timestamp: u64,
    pub period: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct GroupInfo {
    pub name: String,
    pub admin: Address,
    pub asset: Address,
    pub total_contributions: i128,
    pub member_count: u32,
    pub is_active: bool,
}

/// ─── Contract ────────────────────────────────────────────────────────────────

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialize a new cooperative treasury group.
    pub fn initialize(
        env: Env,
        admin: Address,
        group_name: String,
        asset: Address,
    ) -> GroupInfo {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::GroupName, &group_name);
        env.storage().instance().set(&DataKey::AssetAddress, &asset);
        env.storage().instance().set(&DataKey::TotalContributions, &0i128);
        env.storage().instance().set(&DataKey::IsActive, &true);
        env.storage().instance().set(&DataKey::Members, &Vec::<Address>::new(&env));

        GroupInfo {
            name: group_name,
            admin,
            asset,
            total_contributions: 0,
            member_count: 0,
            is_active: true,
        }
    }

    /// Add a new member to the cooperative.
    pub fn add_member(env: Env, admin: Address, member: Address) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let mut members: Vec<Address> = env
            .storage().instance()
            .get(&DataKey::Members)
            .unwrap_or(Vec::new(&env));

        if !members.contains(&member) {
            members.push_back(member.clone());
            env.storage().instance().set(&DataKey::Members, &members);
            env.events().publish(
                (Symbol::new(&env, "member_added"),),
                member,
            );
        }
    }

    /// Record a member contribution. Transfers USDC from member to this contract.
    pub fn contribute(env: Env, member: Address, amount: i128, period: u32) {
        member.require_auth();
        Self::require_member(&env, &member);

        if amount <= 0 {
            panic!("amount must be positive");
        }

        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);

        // Transfer from member wallet to this contract
        token_client.transfer(&member, &env.current_contract_address(), &amount);

        // Record contribution
        let record = ContributionRecord {
            member: member.clone(),
            amount,
            timestamp: env.ledger().timestamp(),
            period,
        };

        let mut history: Vec<ContributionRecord> = env
            .storage().persistent()
            .get(&DataKey::Contributions(member.clone()))
            .unwrap_or(Vec::new(&env));
        history.push_back(record);
        env.storage().persistent()
            .set(&DataKey::Contributions(member.clone()), &history);

        // Update total
        let total: i128 = env.storage().instance()
            .get(&DataKey::TotalContributions).unwrap_or(0);
        env.storage().instance()
            .set(&DataKey::TotalContributions, &(total + amount));

        env.events().publish(
            (Symbol::new(&env, "contribution"),),
            (member, amount, period),
        );
    }

    /// Withdraw funds — only callable by admin (e.g. for approved loans or expenses).
    pub fn withdraw(env: Env, admin: Address, to: Address, amount: i128) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&env.current_contract_address(), &to, &amount);

        env.events().publish(
            (Symbol::new(&env, "withdrawal"),),
            (to, amount),
        );
    }

    /// Get current treasury balance.
    pub fn balance(env: Env) -> i128 {
        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);
        token_client.balance(&env.current_contract_address())
    }

    /// Get all members.
    pub fn get_members(env: Env) -> Vec<Address> {
        env.storage().instance()
            .get(&DataKey::Members)
            .unwrap_or(Vec::new(&env))
    }

    /// Get contribution history for a member.
    pub fn get_contributions(env: Env, member: Address) -> Vec<ContributionRecord> {
        env.storage().persistent()
            .get(&DataKey::Contributions(member))
            .unwrap_or(Vec::new(&env))
    }

    /// Get full group info.
    pub fn get_info(env: Env) -> GroupInfo {
        let members: Vec<Address> = env.storage().instance()
            .get(&DataKey::Members)
            .unwrap_or(Vec::new(&env));
        GroupInfo {
            name: env.storage().instance().get(&DataKey::GroupName).unwrap(),
            admin: env.storage().instance().get(&DataKey::Admin).unwrap(),
            asset: env.storage().instance().get(&DataKey::AssetAddress).unwrap(),
            total_contributions: env.storage().instance()
                .get(&DataKey::TotalContributions).unwrap_or(0),
            member_count: members.len(),
            is_active: env.storage().instance().get(&DataKey::IsActive).unwrap_or(true),
        }
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller {
            panic!("unauthorized: admin only");
        }
    }

    fn require_member(env: &Env, caller: &Address) {
        let members: Vec<Address> = env.storage().instance()
            .get(&DataKey::Members)
            .unwrap_or(Vec::new(env));
        if !members.contains(caller) {
            panic!("unauthorized: members only");
        }
    }
}

/// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{token::Client as TokenClient, token::StellarAssetClient, Env};

    fn setup() -> (Env, TreasuryContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let member = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let asset = env.register_stellar_asset_contract_v2(token_admin.clone());
        let asset_address = asset.address();

        // Fund member
        StellarAssetClient::new(&env, &asset_address)
            .mint(&member, &10_000_0000000i128);

        (env, client, admin, member, asset_address)
    }

    #[test]
    fn test_initialize() {
        let (env, client, admin, _, asset) = setup();
        let info = client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        assert_eq!(info.member_count, 0);
        assert!(info.is_active);
    }

    #[test]
    fn test_add_member_and_contribute() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        let amount = 100_0000000i128; // 100 USDC (7 decimals)
        client.contribute(&member, &amount, &1);

        let balance = client.balance();
        assert_eq!(balance, amount);

        let history = client.get_contributions(&member);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().amount, amount);
    }
}
