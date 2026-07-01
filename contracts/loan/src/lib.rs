//! # Loan Contract
//!
//! Manages the cooperative's member loan lifecycle: requesting, approving,
//! and repaying loans. Loans are funded from the cooperative's treasury and
//! carry a configurable interest rate (default 5% / 500 basis points).
//!
//! ## Overview
//!
//! - **Members** submit loan requests via [`LoanContract::request_loan`].
//! - The **admin** (or a governance contract) approves pending loans via
//!   [`LoanContract::approve_loan`], which disburses funds to the borrower.
//! - **Borrowers** repay loans (partially or fully) via [`LoanContract::repay`].
//!   Once total repaid (principal + interest) meets the required amount the
//!   loan status transitions to `Repaid`.
//!
//! Loan statuses follow a state machine:
//! `Pending → Approved → Repaid` or `Pending → Rejected`.
//! Loans past their repayment deadline may be marked `Defaulted`.
//!
//! ## Storage
//!
//! All loan records are stored in instance storage as a `Vec<Loan>`. A
//! monotonically increasing counter tracks loan IDs.

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
    /// Initialize the loan contract with an admin, treasury reference, and
    /// asset token address.
    ///
    /// Must be called exactly once before any other function. Sets the
    /// initial loan counter to zero.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    ///
    /// - If the contract has already been initialized (`"already initialized"`).
    ///
    /// # Events
    ///
    /// None emitted directly by this function.
    ///
    /// # Return value
    ///
    /// Returns `()`.
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
    /// Creates a new [`Loan`] in `Pending` status with a 5% interest rate
    /// (500 bps) and a repayment deadline calculated from `repayment_days`.
    /// The loan ID is returned and can be used in subsequent approve/repay
    /// calls.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `borrower`.
    ///
    /// # Panics
    ///
    /// - If `amount` is less than or equal to zero (`"amount must be positive"`).
    ///
    /// # Events
    ///
    /// - `"loan_requested"` — emitted with `(id, borrower, amount)`.
    ///
    /// # Return value
    ///
    /// Returns `u32` — the newly assigned loan ID (monotonically increasing).
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
    ///
    /// Moves the loan from `Pending` to `Approved`, records the approval
    /// timestamp, and transfers `amount` of the configured token from the
    /// loan contract to the borrower. The funds are assumed to have been
    /// pre-funded into the loan contract by the treasury.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`. Caller must also be the
    /// registered admin of this contract.
    ///
    /// # Panics
    ///
    /// - If the caller is not the admin (`"unauthorized"`).
    /// - If the loan is not in `Pending` status (`"loan is not pending"`).
    /// - If the loan ID does not exist (`"loan not found"`).
    /// - If the token transfer fails (insufficient balance in contract).
    ///
    /// # Events
    ///
    /// - `"loan_approved"` — emitted with `(loan_id, borrower, amount)`.
    ///
    /// # Return value
    ///
    /// Returns `()`.
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
    ///
    /// Pulls `amount` of the configured token from the borrower into the
    /// loan contract and increments `amount_repaid`. If the total repaid
    /// (including interest) meets or exceeds the required amount, the
    /// loan status transitions to `Repaid`.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `borrower`. Caller must be the original
    /// borrower of this loan.
    ///
    /// # Panics
    ///
    /// - If the caller is not the borrower of this loan (`"not the borrower"`).
    /// - If the loan is not in `Approved` status (`"loan not active"`).
    /// - If the loan ID does not exist (`"loan not found"`).
    /// - If the token transfer fails (insufficient borrower balance).
    ///
    /// # Events
    ///
    /// - `"loan_repaid"` — emitted with `(loan_id, borrower, amount, status)`.
    ///
    /// # Return value
    ///
    /// Returns `()`.
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
    /// Returns the full list of loans ever submitted to this contract,
    /// regardless of status.
    ///
    /// # Authorization
    ///
    /// None required — this is a read-only public query.
    ///
    /// # Panics
    ///
    /// None.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Return value
    ///
    /// Returns `Vec<Loan>` — all loan records, or an empty vector if no
    /// loans have been requested.
    pub fn get_loans(env: Env) -> Vec<Loan> {
        env.storage().instance()
            .get(&DataKey::Loans)
            .unwrap_or(Vec::new(&env))
    }

    /// Get a single loan by ID.
    ///
    /// Looks up the loan with the given `loan_id` and returns its full
    /// [`Loan`] struct.
    ///
    /// # Authorization
    ///
    /// None required — this is a read-only public query.
    ///
    /// # Panics
    ///
    /// - If the loan ID does not exist (`"loan not found"`).
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Return value
    ///
    /// Returns [`Loan`] — the requested loan record.
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
