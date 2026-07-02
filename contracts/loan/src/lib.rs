#![no_std]
//! Loan lifecycle contract for CoopFinance groups.
//!
//! Stores loan requests, lets the configured admin approve and disburse loans,
//! accepts borrower repayments, and exposes read-only loan views.

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
    /// Initializes the loan contract with its admin, treasury, and asset.
    ///
    /// # Authorization
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    /// Panics if the contract has already been initialized.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns nothing.
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

    /// Records a member loan request and returns its new loan id.
    ///
    /// # Authorization
    /// Requires authorization from `borrower`.
    ///
    /// # Panics
    /// Panics if `amount` is not positive.
    ///
    /// # Events
    /// Emits `loan_requested` with `(loan_id, borrower, amount)`.
    ///
    /// # Returns
    /// Returns the newly assigned loan id.
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

    /// Approves a pending loan and transfers the principal to the borrower.
    ///
    /// # Authorization
    /// Requires authorization from `admin`, which must match the stored admin.
    ///
    /// # Panics
    /// Panics if `admin` is not the stored admin, if `loan_id` does not exist,
    /// if the loan is not pending, or if the token transfer fails.
    ///
    /// # Events
    /// Emits `loan_approved` with `(loan_id, borrower, amount)`.
    ///
    /// # Returns
    /// Returns nothing.
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

    /// Records a borrower repayment and marks the loan repaid once fully paid.
    ///
    /// # Authorization
    /// Requires authorization from `borrower`, which must match the loan
    /// borrower.
    ///
    /// # Panics
    /// Panics if `loan_id` does not exist, if `borrower` is not the borrower,
    /// if the loan is not approved, or if the token transfer fails.
    ///
    /// # Events
    /// Emits `loan_repaid` with `(loan_id, borrower, amount, status)`.
    ///
    /// # Returns
    /// Returns nothing.
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

    /// Returns all loans currently stored by the contract.
    ///
    /// # Authorization
    /// No authorization is required.
    ///
    /// # Panics
    /// Does not panic.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns a vector of all loan records, or an empty vector if none exist.
    pub fn get_loans(env: Env) -> Vec<Loan> {
        env.storage().instance()
            .get(&DataKey::Loans)
            .unwrap_or(Vec::new(&env))
    }

    /// Returns a single loan by id.
    ///
    /// # Authorization
    /// No authorization is required.
    ///
    /// # Panics
    /// Panics if no loan with `loan_id` exists.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns the matching loan record.
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
