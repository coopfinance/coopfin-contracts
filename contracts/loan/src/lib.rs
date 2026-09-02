//! Loan contract for Soroban-based cooperative lending.
//!
//! Allows members to request loans, which are approved and disbursed by
//! the admin (or governance contract). Borrowers repay the contract
//! directly, and loan status is tracked through a state machine
//! ([`LoanStatus`]).
//!
//! The contract is `no_std` and Soroban-targeted.
//!
//! # Events
//!
//! - `loan_requested` — emitted when a member submits a loan request.
//! - `loan_approved` — emitted when a loan is approved and disbursed.
//! - `loan_repaid` — emitted when a repayment is recorded.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String, Symbol, Vec};

/// Storage keys for the loan contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address (set at initialization).
    Admin,
    /// Address of the treasury contract that funds disbursements.
    TreasuryContract,
    /// The token asset used for loan disbursements and repayments.
    AssetAddress,
    /// Persistent vector of all [`Loan`] records.
    Loans,
    /// Monotonically increasing loan counter.
    LoanCounter,
}

/// Lifecycle status of a loan.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LoanStatus {
    /// Loan request awaiting approval (not yet disbursed).
    Pending,
    /// Loan has been approved and funds disbursed.
    Approved,
    /// Loan has been fully repaid.
    Repaid,
    /// Loan was rejected.
    Rejected,
    /// Loan is past due and not fully repaid.
    Defaulted,
}

/// A single loan record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Loan {
    /// Unique loan identifier.
    pub id: u32,
    /// The member who requested the loan.
    pub borrower: Address,
    /// Principal amount of the loan, in the asset's minor units.
    pub amount: i128,
    /// Interest rate in basis points (e.g. 500 = 5%).
    pub interest_bps: u32,
    /// Ledger timestamp by which the loan must be repaid.
    pub repayment_due: u64,
    /// Total amount repaid so far.
    pub amount_repaid: i128,
    /// Current lifecycle status of the loan.
    pub status: LoanStatus,
    /// Human-readable purpose of the loan.
    pub purpose: String,
    /// Ledger timestamp when the loan was requested.
    pub requested_at: u64,
    /// Ledger timestamp when the loan was approved (0 if not yet approved).
    pub approved_at: u64,
}

#[contract]
pub struct LoanContract;

#[contractimpl]
impl LoanContract {
    /// Initialize the loan contract with admin, treasury, and asset.
    ///
    /// # Authorization
    ///
    /// The `admin` must authenticate (via `require_auth`).
    ///
    /// # Panics
    ///
    /// Panics if the contract has already been initialized.
    ///
    /// # Events
    ///
    /// Emits no events directly; initialization is a one-time setup step.
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
    ///
    /// # Authorization
    ///
    /// The `borrower` must authenticate (via `require_auth`).
    ///
    /// # Panics
    ///
    /// Panics if `amount` is zero or negative.
    ///
    /// # Events
    ///
    /// Emits `loan_requested` with `(id, borrower, amount)`.
    ///
    /// # Returns
    ///
    /// The new loan's ID.
    pub fn request_loan(
        env: Env,
        borrower: Address,
        amount: i128,
        purpose: String,
        repayment_days: u32,
    ) -> u32 {
        borrower.require_auth();
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::LoanCounter)
            .unwrap_or(0);
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

        let mut loans: Vec<Loan> = env
            .storage()
            .instance()
            .get(&DataKey::Loans)
            .unwrap_or(Vec::new(&env));
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
    ///
    /// # Authorization
    ///
    /// The `admin` must authenticate and be the current admin of the contract.
    ///
    /// # Panics
    ///
    /// - Panics if `admin` is not the contract's admin.
    /// - Panics if the loan is not in `Pending` status.
    ///
    /// # Events
    ///
    /// Emits `loan_approved` with `(loan_id, borrower, amount)`.
    pub fn approve_loan(env: Env, admin: Address, loan_id: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let mut loans: Vec<Loan> = env.storage().instance().get(&DataKey::Loans).unwrap();

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
    ///
    /// # Authorization
    ///
    /// The `borrower` must authenticate (via `require_auth`).
    ///
    /// # Panics
    ///
    /// - Panics if `borrower` is not the loan's borrower.
    /// - Panics if the loan is not in `Approved` status.
    ///
    /// # Events
    ///
    /// Emits `loan_repaid` with `(loan_id, borrower, amount, status)`.
    pub fn repay(env: Env, borrower: Address, loan_id: u32, amount: i128) {
        borrower.require_auth();

        let mut loans: Vec<Loan> = env.storage().instance().get(&DataKey::Loans).unwrap();

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

    /// Get all loans.
    ///
    /// Read-only — no auth required.
    ///
    /// # Returns
    ///
    /// A vector of all [`Loan`] records, empty if none exist.
    pub fn get_loans(env: Env) -> Vec<Loan> {
        env.storage()
            .instance()
            .get(&DataKey::Loans)
            .unwrap_or(Vec::new(&env))
    }

    /// Get a single loan by ID.
    ///
    /// Read-only — no auth required.
    ///
    /// # Arguments
    ///
    /// * `loan_id` — the ID of the loan to retrieve.
    ///
    /// # Panics
    ///
    /// Panics if no loan with the given ID exists.
    ///
    /// # Returns
    ///
    /// The [`Loan`] record.
    pub fn get_loan(env: Env, loan_id: u32) -> Loan {
        let loans: Vec<Loan> = env.storage().instance().get(&DataKey::Loans).unwrap();
        let idx = Self::find_loan_idx(&loans, loan_id);
        loans.get(idx).unwrap()
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller {
            panic!("unauthorized");
        }
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
