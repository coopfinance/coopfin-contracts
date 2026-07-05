#![no_std]

//! Member-loan contract for a cooperative.
//!
//! [`LoanContract`] is the lifecycle owner of every loan in the coop:
//! a member calls [`request_loan`] to file an application, governance
//! (or the admin) calls [`approve_loan`] to disburse funds from the
//! treasury, and the borrower repays in one or more calls to [`repay`].
//!
//! All amounts are denominated in token base units (7 decimals on USDC),
//! and interest is computed in basis points against the principal only.

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec, String,
};

/// Storage keys for [`LoanContract`].
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address authorized to approve loans.
    Admin,
    /// Address of the sibling treasury contract.
    TreasuryContract,
    /// Address of the SAC22 token used for disbursement and repayment.
    AssetAddress,
    /// Append-only log of every [`Loan`] created on this contract.
    Loans,
    /// Monotonically increasing counter used to assign loan IDs.
    LoanCounter,
}

/// Status of a loan through its lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LoanStatus {
    /// Awaiting approval vote.
    Pending,
    /// Disbursed to the borrower.
    Approved,
    /// Fully repaid (principal + interest).
    Repaid,
    /// Rejected by governance.
    Rejected,
    /// Past due date, not repaid.
    Defaulted,
}

/// Single loan record. Fields are set incrementally as the loan moves
/// through the [`LoanStatus`] lifecycle.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Loan {
    /// Auto-incremented ID assigned at request time.
    pub id: u32,
    /// Address of the borrower; principal is disbursed to this account.
    pub borrower: Address,
    /// Principal amount in token base units.
    pub amount: i128,
    /// Interest rate in basis points (500 = 5.00%).
    pub interest_bps: u32,
    /// Ledger timestamp at which repayment is due.
    pub repayment_due: u64,
    /// Running total of all repayments applied to this loan.
    pub amount_repaid: i128,
    /// Current status; see [`LoanStatus`].
    pub status: LoanStatus,
    /// Human-readable purpose supplied by the borrower.
    pub purpose: String,
    /// Ledger timestamp when the loan was requested.
    pub requested_at: u64,
    /// Ledger timestamp when the loan was approved (0 until then).
    pub approved_at: u64,
}

#[contract]
pub struct LoanContract;

#[contractimpl]
impl LoanContract {
    /// Initialize the loan contract.
    ///
    /// Stores the admin, treasury contract address, asset address, and an
    /// empty loan counter + log. Must be called exactly once.
    ///
    /// # Panics
    /// Panics with `"already initialized"` if the contract already has an
    /// admin on file.
    ///
    /// # Authorization
    /// Requires auth from `admin`.
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

    /// Internal: assert that `caller` is the admin registered at [`initialize`].
    ///
    /// # Panics
    /// Panics with `"unauthorized"` when `caller` does not match.
    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }

    /// Internal: linear search for a loan by ID in the on-chain log.
    ///
    /// # Panics
    /// Panics with `"loan not found"` when no loan has the supplied ID.
    fn find_loan_idx(loans: &Vec<Loan>, id: u32) -> u32 {
        for i in 0..loans.len() {
            if loans.get(i).unwrap().id == id {
                return i;
            }
        }
        panic!("loan not found");
    }
}
