#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec, String,
};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TreasuryContract,
    AssetAddress,
    Loans,
    LoanCounter,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LoanStatus {
    Pending,   // Awaiting approval vote
    Approved,  // Disbursed
    Repaid,    // Fully repaid
    Rejected,  // Rejected by governance
    Defaulted, // Past due date, not repaid
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Loan {
    pub id: u32,
    pub borrower: Address,
    pub amount: i128,
    pub interest_bps: u32,      // basis points, e.g. 500 = 5%
    pub repayment_due: u64,     // ledger timestamp deadline
    pub amount_repaid: i128,
    pub status: LoanStatus,
    pub purpose: String,
    pub requested_at: u64,
    pub approved_at: u64,
}

#[contract]
pub struct LoanContract;

#[contractimpl]
impl LoanContract {
    pub fn initialize(env: Env, admin: Address, treasury: Address, asset: Address) {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::AssetAddress, &asset);
        env.storage().instance().set(&DataKey::LoanCounter, &0u32);
        env.storage().instance().set(&DataKey::Loans, &Vec::<Loan>::new(&env));
    }

    /// Member submits a loan request.
    pub fn request_loan(
        env: Env,
        borrower: Address,
        amount: i128,
        purpose: String,
        repayment_days: u32,
    ) -> u32 {
        borrower.require_auth();
        if amount <= 0 { panic!("amount must be positive"); }

        let counter: u32 = env.storage().instance()
            .get(&DataKey::LoanCounter).unwrap_or(0);
        let id = counter + 1;

        let seconds_per_day: u64 = 86_400;
        let due = env.ledger().timestamp() + (repayment_days as u64 * seconds_per_day);

        let loan = Loan {
            id,
            borrower: borrower.clone(),
            amount,
            interest_bps: 500, // 5% flat — governance can change this
            repayment_due: due,
            amount_repaid: 0,
            status: LoanStatus::Pending,
            purpose,
            requested_at: env.ledger().timestamp(),
            approved_at: 0,
        };

        let mut loans: Vec<Loan> = env.storage().instance()
            .get(&DataKey::Loans).unwrap_or(Vec::new(&env));
        loans.push_back(loan);
        env.storage().instance().set(&DataKey::Loans, &loans);
        env.storage().instance().set(&DataKey::LoanCounter, &id);

        env.events().publish(
            (Symbol::new(&env, "loan_requested"),),
            (id, borrower, amount),
        );
        id
    }

    /// Admin (or governance contract) approves a loan and disburses funds.
    pub fn approve_loan(env: Env, admin: Address, loan_id: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let mut loans: Vec<Loan> = env.storage().instance()
            .get(&DataKey::Loans).unwrap();

        let idx = Self::find_loan_idx(&loans, loan_id);
        let mut loan = loans.get(idx).unwrap();

        if loan.status != LoanStatus::Pending {
            panic!("loan is not pending");
        }

        loan.status = LoanStatus::Approved;
        loan.approved_at = env.ledger().timestamp();
        loans.set(idx, loan.clone());
        env.storage().instance().set(&DataKey::Loans, &loans);

        // Disburse from treasury
        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(
            &env.current_contract_address(),
            &loan.borrower,
            &loan.amount,
        );

        env.events().publish(
            (Symbol::new(&env, "loan_approved"),),
            (loan_id, loan.borrower, loan.amount),
        );
    }

    /// Borrower repays (partial or full).
    pub fn repay(env: Env, borrower: Address, loan_id: u32, amount: i128) {
        borrower.require_auth();

        let mut loans: Vec<Loan> = env.storage().instance()
            .get(&DataKey::Loans).unwrap();

        let idx = Self::find_loan_idx(&loans, loan_id);
        let mut loan = loans.get(idx).unwrap();

        if loan.borrower != borrower {
            panic!("not the borrower");
        }
        if loan.status != LoanStatus::Approved {
            panic!("loan not active");
        }

        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&borrower, &env.current_contract_address(), &amount);

        loan.amount_repaid += amount;

        let total_due = loan.amount + (loan.amount * loan.interest_bps as i128 / 10_000);
        if loan.amount_repaid >= total_due {
            loan.status = LoanStatus::Repaid;
        }

        loans.set(idx, loan.clone());
        env.storage().instance().set(&DataKey::Loans, &loans);

        env.events().publish(
            (Symbol::new(&env, "loan_repaid"),),
            (loan_id, borrower, amount, loan.status),
        );
    }

    /// Marks a loan as defaulted if it is past due and has pending balance.
    ///
    /// # Authorization
    /// Anyone can call this function (community enforcement).
    ///
    /// # Arguments
    /// * `loan_id` - The ID of the loan to mark as defaulted
    ///
    /// # Panics
    /// - If loan does not exist
    /// - If loan is not in `Approved` status
    /// - If loan is not past due (`repayment_due` > current ledger timestamp)
    /// - If loan has been fully repaid
    ///
    /// # Events
    /// Emits `loan_defaulted` with loan_id, borrower, and pending_amount.
    pub fn mark_defaulted(env: Env, loan_id: u32) {
        // 1. Obtener todos los préstamos
        let mut loans: Vec<Loan> = env.storage().instance()
            .get(&DataKey::Loans).unwrap_or_else(|| panic!("no loans found"));

        // 2. Encontrar el índice del préstamo
        let idx = Self::find_loan_idx(&loans, loan_id);
        let mut loan = loans.get(idx).unwrap();

        // 3. Validar que el préstamo está en estado Approved
        if loan.status != LoanStatus::Approved {
            panic!("loan must be in Approved status");
        }

        // 4. Validar que el préstamo está vencido
        let current_timestamp = env.ledger().timestamp();
        if current_timestamp <= loan.repayment_due {
            panic!("loan is not past due");
        }

        // 5. Validar que hay saldo pendiente
        let total_due = loan.amount + (loan.amount * loan.interest_bps as i128 / 10_000);
        if loan.amount_repaid >= total_due {
            panic!("loan has been fully repaid");
        }

        // 6. Calcular el monto pendiente
        let pending_amount = total_due - loan.amount_repaid;

        // 7. Actualizar el estado a Defaulted
        loan.status = LoanStatus::Defaulted;

        // 8. Guardar el préstamo actualizado
        loans.set(idx, loan.clone());
        env.storage().instance().set(&DataKey::Loans, &loans);

        // 9. Emitir el evento
        env.events().publish(
            (Symbol::new(&env, "loan_defaulted"),),
            (loan_id, loan.borrower, pending_amount),
        );

        // 10. Extender TTL del storage de instancia (100 ledgers)
        env.storage().instance().extend_ttl(100, 100);
    }

    /// Get all loans.
    pub fn get_loans(env: Env) -> Vec<Loan> {
        env.storage().instance()
            .get(&DataKey::Loans)
            .unwrap_or(Vec::new(&env))
    }

    /// Get a single loan by ID.
    pub fn get_loan(env: Env, loan_id: u32) -> Loan {
        let loans: Vec<Loan> = env.storage().instance()
            .get(&DataKey::Loans).unwrap();
        let idx = Self::find_loan_idx(&loans, loan_id);
        loans.get(idx).unwrap()
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }

    fn find_loan_idx(loans: &Vec<Loan>, id: u32) -> u32 {
        for i in 0..loans.len() {
            if loans.get(i).unwrap().id == id {
                return i;
            }
        }
        panic!("loan not found");
    }
}

/// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{token::Client as TokenClient, token::StellarAssetClient, Env};

    fn setup() -> (Env, LoanContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, LoanContract);
        let client = LoanContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let borrower = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let asset = env.register_stellar_asset_contract_v2(token_admin.clone());
        let asset_address = asset.address();

        // Fund borrower
        StellarAssetClient::new(&env, &asset_address)
            .mint(&borrower, &10_000_0000000i128);

        // Initialize contract
        client.initialize(&admin, &admin, &asset_address);

        (env, client, admin, borrower, asset_address)
    }

    fn create_approved_loan(
        env: &Env,
        client: &LoanContractClient<'static>,
        admin: &Address,
        borrower: &Address,
        asset: &Address,
    ) -> u32 {
        // Request loan
        let loan_id = client.request_loan(
            borrower,
            &1_000_0000000i128,
            &String::from_str(env, "Test loan"),
            &30, // 30 days
        );

        // Approve loan (disburses funds)
        client.approve_loan(admin, &loan_id);

        loan_id
    }

    #[test]
    fn test_initialize() {
        let (env, client, admin, _, asset) = setup();
        let info = client.initialize(&admin, &admin, &asset);
        // No panic means success
    }

    #[test]
    fn test_request_loan() {
        let (env, client, admin, borrower, asset) = setup();
        let loan_id = client.request_loan(
            &borrower,
            &1_000_0000000i128,
            &String::from_str(&env, "Test loan"),
            &30,
        );
        assert_eq!(loan_id, 1);
    }

    #[test]
    fn test_approve_loan() {
        let (env, client, admin, borrower, asset) = setup();
        let loan_id = client.request_loan(
            &borrower,
            &1_000_0000000i128,
            &String::from_str(&env, "Test loan"),
            &30,
        );
        client.approve_loan(&admin, &loan_id);
        let loan = client.get_loan(&loan_id);
        assert_eq!(loan.status, LoanStatus::Approved);
    }

    #[test]
    fn test_repay_loan() {
        let (env, client, admin, borrower, asset) = setup();
        let loan_id = create_approved_loan(&env, &client, &admin, &borrower, &asset);

        // Repay loan
        let total_due = 1_000_0000000i128 + (1_000_0000000i128 * 500 / 10_000); // 5% interest
        client.repay(&borrower, &loan_id, &total_due);

        let loan = client.get_loan(&loan_id);
        assert_eq!(loan.status, LoanStatus::Repaid);
    }

    // ── mark_defaulted tests ────────────────────────────────────────────────────

    #[test]
    fn test_mark_defaulted_success() {
        let (env, client, admin, borrower, asset) = setup();
        let loan_id = create_approved_loan(&env, &client, &admin, &borrower, &asset);

        // Advance time past repayment_due
        env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

        // Mark as defaulted
        client.mark_defaulted(&loan_id);

        // Verify status
        let loan = client.get_loan(&loan_id);
        assert_eq!(loan.status, LoanStatus::Defaulted);
    }

    #[test]
    #[should_panic(expected = "loan is not past due")]
    fn test_mark_defaulted_not_past_due() {
        let (env, client, admin, borrower, asset) = setup();
        let loan_id = create_approved_loan(&env, &client, &admin, &borrower, &asset);

        // Don't advance time -> loan is not past due
        client.mark_defaulted(&loan_id);
    }

    #[test]
    #[should_panic(expected = "loan must be in Approved status")]
    fn test_mark_defaulted_already_repaid() {
        let (env, client, admin, borrower, asset) = setup();
        let loan_id = create_approved_loan(&env, &client, &admin, &borrower, &asset);

        // Repay the loan
        let total_due = 1_000_0000000i128 + (1_000_0000000i128 * 500 / 10_000);
        client.repay(&borrower, &loan_id, &total_due);

        // Advance time
        env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

        // Try to mark as defaulted (should fail because already repaid)
        client.mark_defaulted(&loan_id);
    }

    #[test]
    #[should_panic(expected = "loan not found")]
    fn test_mark_defaulted_loan_not_found() {
        let (env, client, _, _, _) = setup();
        client.mark_defaulted(&999);
    }

    // ── edge cases ──────────────────────────────────────────────────────────────

    #[test]
    #[should_panic]
    fn test_approve_nonexistent_loan() {
        let (env, client, admin, _, asset) = setup();
        client.approve_loan(&admin, &999);
    }

    #[test]
    #[should_panic]
    fn test_repay_nonexistent_loan() {
        let (env, client, _, borrower, _) = setup();
        client.repay(&borrower, &999, &100);
    }
}