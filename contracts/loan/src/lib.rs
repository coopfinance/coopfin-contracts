#![no_std]
//! # CoopFin Loan Contract
//!
//! A Soroban smart contract for managing member loans within a cooperative.
//! Members can request loans against the treasury pool; an admin (or governance
//! contract) approves and disburses funds; borrowers repay with interest.
//!
//! ## Lifecycle
//!
//! 1. `request_loan` — member submits a loan request
//! 2. `approve_loan` — admin disburses funds from the treasury
//! 3. `repay` — borrower makes partial or full repayment
//!
//! ## Usage
//!
//! Deploy to the Stellar Soroban network with `soroban contract deploy`.

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec, String,
};

/// Storage keys for the loan contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address authorized to approve/reject loans.
    Admin,
    /// Address of the treasury contract (source of funds for disbursement).
    TreasuryContract,
    /// The Stellar asset contract address used for loans.
    AssetAddress,
    /// All loan records.
    Loans,
    /// Auto-incrementing loan ID counter.
    LoanCounter,
}

/// The lifecycle status of a loan.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LoanStatus {
    /// Awaiting approval vote by governance / admin.
    Pending,
    /// Funds have been disbursed to the borrower.
    Approved,
    /// Loan has been fully repaid (principal + interest).
    Repaid,
    /// Rejected by governance / admin.
    Rejected,
    /// Past the due date without full repayment.
    Defaulted,
}

/// A single loan record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Loan {
    /// Unique loan identifier (auto-incremented).
    pub id: u32,
    /// The Stellar address of the borrower.
    pub borrower: Address,
    /// Principal amount borrowed (in asset's smallest unit).
    pub amount: i128,
    /// Annual interest rate in basis points (e.g. 500 = 5%).
    pub interest_bps: u32,
    /// Ledger timestamp of the repayment deadline.
    pub repayment_due: u64,
    /// Cumulative amount repaid so far.
    pub amount_repaid: i128,
    /// Current status of the loan.
    pub status: LoanStatus,
    /// Human-readable purpose description.
    pub purpose: String,
    /// Ledger timestamp when the loan was requested.
    pub requested_at: u64,
    /// Ledger timestamp when the loan was approved (0 if not approved).
    pub approved_at: u64,
}

#[contract]
pub struct LoanContract;

#[contractimpl]
impl LoanContract {
    /// Initialize the loan contract.
    ///
    /// Sets up the admin, treasury contract address, asset, and initial
    /// loan storage. Can only be called once.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Panics
    ///
    /// Panics if the contract has already been initialized.
    ///
    /// # Events
    ///
    /// No events emitted.
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

    /// Submit a loan request as a member.
    ///
    /// Creates a new [`Loan`] record in `Pending` status with a flat 5%
    /// interest rate. The repayment deadline is calculated as
    /// `current_timestamp + repayment_days * 86400`.
    ///
    /// # Authorization
    ///
    /// Requires `borrower.require_auth()`.
    ///
    /// # Panics
    ///
    /// Panics if `amount <= 0`.
    ///
    /// # Events
    ///
    /// Emits `loan_requested` with `(id, borrower, amount)`.
    ///
    /// # Returns
    ///
    /// The auto-assigned loan ID as `u32`.
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

    /// Approve a pending loan and disburse funds to the borrower.
    ///
    /// Transfers the loan principal from this contract (which should have
    /// received funds from the treasury) to the borrower's wallet.  The
    /// loan status is updated to `Approved` and the `approved_at`
    /// timestamp is recorded.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()` and that the caller is the stored
    /// admin address.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The caller is not the admin
    /// - The loan ID does not exist
    /// - The loan is not in `Pending` status
    ///
    /// # Events
    ///
    /// Emits `loan_approved` with `(loan_id, borrower, amount)`.
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

    /// Repay (partially or fully) an approved loan.
    ///
    /// Transfers `amount` from the borrower to this contract. If the total
    /// repaid reaches or exceeds `principal + interest`, the loan is marked
    /// `Repaid`.
    ///
    /// # Authorization
    ///
    /// Requires `borrower.require_auth()`. The caller must match the loan's
    /// `borrower` field.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The caller is not the loan's borrower
    /// - The loan ID does not exist
    /// - The loan is not in `Approved` status
    ///
    /// # Events
    ///
    /// Emits `loan_repaid` with `(loan_id, borrower, amount, status)`.
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

    /// Get all loans.
    ///
    /// # Authorization
    ///
    /// None — read-only query.
    ///
    /// # Panics
    ///
    /// Never panics — returns an empty `Vec` if no loans exist.
    ///
    /// # Returns
    ///
    /// A `Vec<Loan>` of all loan records.
    pub fn get_loans(env: Env) -> Vec<Loan> {
        env.storage().instance()
            .get(&DataKey::Loans)
            .unwrap_or(Vec::new(&env))
    }

    /// Get a single loan by ID.
    ///
    /// # Authorization
    ///
    /// None — read-only query.
    ///
    /// # Panics
    ///
    /// Panics if the loan ID does not exist.
    ///
    /// # Returns
    ///
    /// The [`Loan`] record matching the given ID.
    pub fn get_loan(env: Env, loan_id: u32) -> Loan {
        let loans: Vec<Loan> = env.storage().instance()
            .get(&DataKey::Loans).unwrap();
        let idx = Self::find_loan_idx(&loans, loan_id);
        loans.get(idx).unwrap()
    }

    /// Assert that `caller` is the stored admin, panicking otherwise.
    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }

    /// Find the index of a loan by ID in a `Vec<Loan>`, panicking if not found.
    fn find_loan_idx(loans: &Vec<Loan>, id: u32) -> u32 {
        for i in 0..loans.len() {
            if loans.get(i).unwrap().id == id {
                return i;
            }
        }
        panic!("loan not found");
    }
}
