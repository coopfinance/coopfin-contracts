#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec, String,
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

/// Complete snapshot of a single member, aggregated in one read-only call so the
/// frontend dashboard does not have to combine `get_members` and
/// `get_contributions` client-side (multiple RPC round-trips per member).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MemberSummary {
    pub address: Address,
    pub is_member: bool,
    pub total_contributed: i128,
    pub contribution_count: u32,
    pub last_period: u32,
    pub last_contributed_at: u64,
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

    /// Removes a member from the treasury.
    ///
    /// # Authorization
    /// Only the admin can remove members.
    ///
    /// # Arguments
    /// * `admin` - The admin's address
    /// * `member` - The member to remove
    /// * `force` - If true, bypass loan balance check
    ///
    /// # Panics
    /// Panics if `member` is not found or has pending loans (unless `force` is true).
    ///
    /// # Events
    /// Emits `member_removed` with member address and timestamp.
    pub fn remove_member(env: Env, admin: Address, member: Address, force: bool) {
        // 1. Auth: solo el admin puede llamar
        admin.require_auth();
        Self::require_admin(&env, &admin);

        // 2. Obtener el vector de miembros del storage de instancia
        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .unwrap_or_else(|| panic!("no members found"));

        // 3. Validar que el miembro existe
        if !members.contains(&member) {
            panic!("member not found");
        }

        // 4. Verificar si tiene préstamos pendientes (a menos que force sea true)
        if !force {
            // Por ahora, asumimos que no hay préstamos.
            // En una implementación real, consultarías el contrato de préstamos.
            // Este es un placeholder para la lógica de verificación de préstamos.
            let has_loan = false; // TODO: Implementar verificación real con LoanContract
            if has_loan {
                panic!("member has pending loan, use force=true to override");
            }
        }

        // 5. Eliminar el miembro del vector (reconstruir el vector)
        let mut new_members: Vec<Address> = Vec::new(&env);
        for m in members.iter() {
            if m != &member {
                new_members.push_back(m.clone());
            }
        }

        // 6. Guardar el nuevo vector en storage
        env.storage()
            .instance()
            .set(&DataKey::Members, &new_members);

        // 7. Emitir el evento
        env.events().publish(
            (Symbol::new(&env, "member_removed"),),
            (member, env.ledger().timestamp()),
        );

        // 8. Extender TTL del storage de instancia (100 ledgers)
        env.storage().instance().extend_ttl(100, 100);
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

    /// Aggregate a member's full picture in a single read-only call.
    ///
    /// Combines membership status with stats derived from the member's stored
    /// contribution history: total contributed, number of contributions, and the
    /// period / ledger timestamp of the most recent one. This lets the dashboard
    /// render a member row with one RPC instead of `get_members` +
    /// `get_contributions`.
    ///
    /// Read-only — no auth required. An unknown address (or a member who has not
    /// contributed yet) returns zeroed stats and never panics; `is_member`
    /// reflects whether the address is in the members list regardless.
    pub fn get_member_summary(env: Env, member: Address) -> MemberSummary {
        let members: Vec<Address> = env
            .storage().instance()
            .get(&DataKey::Members)
            .unwrap_or(Vec::new(&env));
        let is_member = members.contains(&member);

        let history: Vec<ContributionRecord> = env
            .storage().persistent()
            .get(&DataKey::Contributions(member.clone()))
            .unwrap_or(Vec::new(&env));

        let contribution_count = history.len();
        let mut total_contributed: i128 = 0;
        for record in history.iter() {
            total_contributed += record.amount;
        }

        let (last_period, last_contributed_at) = if contribution_count > 0 {
            let last = history.get(contribution_count - 1).unwrap();
            (last.period, last.timestamp)
        } else {
            (0, 0)
        };

        MemberSummary {
            address: member,
            is_member,
            total_contributed,
            contribution_count,
            last_period,
            last_contributed_at,
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

    // ── initialize edge cases ────────────────────────────────────────────────

    #[test]
    #[should_panic]
    fn test_double_initialize() {
        let (env, client, admin, _, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.initialize(&admin, &String::from_str(&env, "Test Coop 2"), &asset);
    }

    // ── add_member edge cases ────────────────────────────────────────────────

    #[test]
    #[should_panic]
    fn test_add_member_unauthorized() {
        let (env, client, admin, member, asset) = setup();
        let non_admin = Address::generate(&env);
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&non_admin, &member);
    }

    #[test]
    fn test_add_member_duplicate_is_idempotent() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);
        client.add_member(&admin, &member);
        assert_eq!(client.get_members().len(), 1);
    }

    // ── remove_member tests ──────────────────────────────────────────────────

    #[test]
    fn test_remove_member_happy_path() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        // Verificar que el miembro existe
        let members = client.get_members();
        assert_eq!(members.len(), 1);
        assert_eq!(members.get(0).unwrap(), member);

        // Remover el miembro
        client.remove_member(&admin, &member, &false);

        // Verificar que ya no existe
        let members_after = client.get_members();
        assert_eq!(members_after.len(), 0);
    }

    #[test]
    #[should_panic(expected = "member not found")]
    fn test_remove_nonexistent_member() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        // Intentar remover un miembro que no existe
        client.remove_member(&admin, &member, &false);
    }

    #[test]
    #[should_panic]
    fn test_remove_member_unauthorized() {
        let (env, client, admin, member, asset) = setup();
        let non_admin = Address::generate(&env);
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);
        // Intentar remover con un no-admin
        client.remove_member(&non_admin, &member, &false);
    }

    #[test]
    fn test_remove_member_preserves_contribution_history() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        // Hacer una contribución
        let amount = 100_0000000i128;
        client.contribute(&member, &amount, &1);

        // Remover el miembro
        client.remove_member(&admin, &member, &false);

        // Verificar que el historial de contribuciones se conserva
        let history = client.get_contributions(&member);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().amount, amount);
    }

    // ── contribute edge cases ────────────────────────────────────────────────

    #[test]
    #[should_panic]
    fn test_contribute_zero_amount() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);
        client.contribute(&member, &0i128, &1);
    }

    #[test]
    #[should_panic]
    fn test_contribute_negative_amount() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);
        client.contribute(&member, &-1_0000000i128, &1);
    }

    #[test]
    #[should_panic]
    fn test_contribute_non_member() {
        let (env, client, admin, _, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        let non_member = Address::generate(&env);
        client.contribute(&non_member, &100_0000000i128, &1);
    }

    // ── withdraw happy path + edge cases ────────────────────────────────────

    #[test]
    fn test_withdraw_happy_path() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        let deposit = 500_0000000i128;
        client.contribute(&member, &deposit, &1);
        assert_eq!(client.balance(), deposit);

        let recipient = Address::generate(&env);
        let withdrawal = 200_0000000i128;
        client.withdraw(&admin, &recipient, &withdrawal);

        assert_eq!(client.balance(), deposit - withdrawal);

        let token = TokenClient::new(&env, &asset);
        assert_eq!(token.balance(&recipient), withdrawal);
    }

    #[test]
    #[should_panic]
    fn test_withdraw_unauthorized() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        let non_admin = Address::generate(&env);
        client.withdraw(&non_admin, &member, &100_0000000i128);
    }

    #[test]
    #[should_panic]
    fn test_withdraw_overdraw() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        let deposit = 100_0000000i128;
        client.contribute(&member, &deposit, &1);

        let recipient = Address::generate(&env);
        client.withdraw(&admin, &recipient, &(deposit + 1));
    }

    // ── query functions ──────────────────────────────────────────────────────

    #[test]
    fn test_balance_initial_is_zero() {
        let (env, client, admin, _, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        assert_eq!(client.balance(), 0);
    }

    #[test]
    fn test_get_members_returns_all_added() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);

        assert_eq!(client.get_members().len(), 0);

        client.add_member(&admin, &member);
        let members = client.get_members();
        assert_eq!(members.len(), 1);
        assert_eq!(members.get(0).unwrap(), member);
    }

    #[test]
    fn test_get_contributions_empty_before_any() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        assert_eq!(client.get_contributions(&member).len(), 0);
    }

    #[test]
    fn test_get_contributions_multiple_periods() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        client.contribute(&member, &100_0000000i128, &1);
        client.contribute(&member, &250_0000000i128, &2);

        let history = client.get_contributions(&member);
        assert_eq!(history.len(), 2);
        assert_eq!(history.get(0).unwrap().amount, 100_0000000i128);
        assert_eq!(history.get(0).unwrap().period, 1);
        assert_eq!(history.get(1).unwrap().amount, 250_0000000i128);
        assert_eq!(history.get(1).unwrap().period, 2);
    }

    #[test]
    fn test_get_info_reflects_state() {
        let (env, client, admin, member, asset) = setup();
        let group_name = String::from_str(&env, "My Coop");
        client.initialize(&admin, &group_name, &asset);
        client.add_member(&admin, &member);

        let amount = 300_0000000i128;
        client.contribute(&member, &amount, &1);

        let info = client.get_info();
        assert_eq!(info.member_count, 1);
        assert_eq!(info.total_contributions, amount);
        assert_eq!(info.admin, admin);
        assert_eq!(info.asset, asset);
        assert!(info.is_active);
    }

    // ── multi-member scenario ────────────────────────────────────────────────

    #[test]
    fn test_multiple_members_contribute_independently() {
        let (env, client, admin, member1, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);

        let member2 = Address::generate(&env);
        StellarAssetClient::new(&env, &asset).mint(&member2, &5_000_0000000i128);

        client.add_member(&admin, &member1);
        client.add_member(&admin, &member2);

        let amount1 = 100_0000000i128;
        let amount2 = 400_0000000i128;
        client.contribute(&member1, &amount1, &1);
        client.contribute(&member2, &amount2, &1);

        assert_eq!(client.balance(), amount1 + amount2);
        assert_eq!(client.get_members().len(), 2);

        let info = client.get_info();
        assert_eq!(info.total_contributions, amount1 + amount2);
        assert_eq!(info.member_count, 2);
    }

    // ── timestamp recording ──────────────────────────────────────────────────

    #[test]
    fn test_contribute_records_ledger_timestamp() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        let ts = 1_700_000_000u64;
        env.ledger().with_mut(|l| l.timestamp = ts);
        client.contribute(&member, &100_0000000i128, &1);

        let record = client.get_contributions(&member).get(0).unwrap();
        assert_eq!(record.timestamp, ts);
    }

    // ── get_member_summary ───────────────────────────────────────────────────

    #[test]
    fn test_get_member_summary_known_member_with_contributions() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        let ts = 1_700_000_500u64;
        env.ledger().with_mut(|l| l.timestamp = ts);
        client.contribute(&member, &100_0000000i128, &1);
        client.contribute(&member, &250_0000000i128, &3);

        let summary = client.get_member_summary(&member);
        assert_eq!(summary.address, member);
        assert!(summary.is_member);
        assert_eq!(summary.total_contributed, 350_0000000i128);
        assert_eq!(summary.contribution_count, 2);
        // Reflects the most recent contribution.
        assert_eq!(summary.last_period, 3);
        assert_eq!(summary.last_contributed_at, ts);
    }

    #[test]
    fn test_get_member_summary_unknown_address_no_panic() {
        let (env, client, admin, _, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);

        let stranger = Address::generate(&env);
        let summary = client.get_member_summary(&stranger);
        assert_eq!(summary.address, stranger);
        assert!(!summary.is_member);
        assert_eq!(summary.total_contributed, 0);
        assert_eq!(summary.contribution_count, 0);
        assert_eq!(summary.last_period, 0);
        assert_eq!(summary.last_contributed_at, 0);
    }

    #[test]
    fn test_get_member_summary_member_without_contributions() {
        let (env, client, admin, member, asset) = setup();
        client.initialize(&admin, &String::from_str(&env, "Test Coop"), &asset);
        client.add_member(&admin, &member);

        let summary = client.get_member_summary(&member);
        assert!(summary.is_member);
        assert_eq!(summary.total_contributed, 0);
        assert_eq!(summary.contribution_count, 0);
        assert_eq!(summary.last_period, 0);
        assert_eq!(summary.last_contributed_at, 0);
    }
}