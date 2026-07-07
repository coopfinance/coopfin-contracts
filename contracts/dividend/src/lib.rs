#![no_std]
//! # CoopFin Dividend Contract
//!
//! Manages profit distribution (dividends / interest surplus sharing)
//! for cooperative members. At the end of each cycle, accumulated profit
//! is distributed proportionally to each member's total contributions
//! during that period.
//!
//! ## Lifecycle
//!
//! 1. `distribute` — admin triggers a distribution, specifying the
//!    period and total profit to distribute. Each member receives a
//!    share proportional to their contribution history.
//! 2. `get_distributions` — anyone can query past distribution records.

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec, String,
};

/// Storage keys for the dividend contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address authorized to trigger distributions.
    Admin,
    /// Address of the treasury contract (source of contribution data).
    TreasuryContract,
    /// The Stellar asset contract address used for dividend payments.
    AssetAddress,
    /// All distribution records.
    Distributions,
}

/// A single dividend distribution record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Distribution {
    /// Sequential distribution number (1-based).
    pub id: u32,
    /// Period label (e.g. "Q1 2026", "December 2025").
    pub period: String,
    /// Total profit distributed in this distribution.
    pub total_amount: i128,
    /// Number of members who received a share.
    pub recipient_count: u32,
    /// Ledger timestamp when the distribution was executed.
    pub timestamp: u64,
    /// Member addresses and the amounts they received.
    pub recipients: Vec<(Address, i128)>,
}

#[contract]
pub struct DividendContract;

#[contractimpl]
impl DividendContract {
    /// Initialize the dividend contract.
    ///
    /// Stores the admin, treasury contract address, asset, and an
    /// empty distribution list.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Panics
    ///
    /// No explicit panics — callable once per deploy.
    ///
    /// # Events
    ///
    /// No events emitted.
    pub fn initialize(env: Env, admin: Address, treasury: Address, asset: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::AssetAddress, &asset);
        env.storage().instance().set(&DataKey::Distributions, &Vec::<Distribution>::new(&env));
    }

    /// Trigger a profit distribution.
    ///
    /// Transfers `total_profit` from the treasury contract to this
    /// contract, then disburses proportional shares among `members`
    /// based on each member's `contributed` amount.
    ///
    /// The distribution amount per member is:
    /// ```text
    /// share = total_profit * member_contributed / total_contributions
    /// ```
    ///
    /// If any member contributed 0, they receive 0.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()` and that the caller is the
    /// stored admin address.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `total_contributions == 0`
    /// - `total_profit <= 0`
    /// - The caller is not the admin
    ///
    /// # Events
    ///
    /// Emits `dividend_paid` with `(distribution_id, period)`.
    pub fn distribute(
        env: Env,
        admin: Address,
        period: String,
        total_profit: i128,
        members: Vec<Address>,
        contributed: Vec<i128>,
    ) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        if total_profit <= 0 { panic!("total_profit must be positive"); }

        let total_contributions: i128 = contributed.iter().sum();
        if total_contributions == 0 { panic!("total_contributions cannot be zero"); }

        let asset: Address = env.storage().instance().get(&DataKey::AssetAddress).unwrap();
        let token_client = token::Client::new(&env, &asset);

        // Pull profit from treasury
        let treasury: Address = env.storage().instance().get(&DataKey::TreasuryContract).unwrap();
        token_client.transfer(&treasury, &env.current_contract_address(), &total_profit);

        let mut distribution_id: u32 = 0;
        let mut recipients: Vec<(Address, i128)> = Vec::new(&env);
        let mut recipient_count: u32 = 0;

        for i in 0..members.len() {
            let member = members.get(i).unwrap();
            let member_share = contributed.get(i).unwrap();
            if member_share > 0 {
                let payout = total_profit * member_share / total_contributions;
                if payout > 0 {
                    token_client.transfer(
                        &env.current_contract_address(),
                        &member,
                        &payout,
                    );
                    recipients.push_back((member, payout));
                    recipient_count += 1;
                }
            }
        }

        // Store distribution record
        let mut distributions: Vec<Distribution> = env.storage().instance()
            .get(&DataKey::Distributions).unwrap_or(Vec::new(&env));
        distribution_id = distributions.len() as u32 + 1;

        let record = Distribution {
            id: distribution_id,
            period,
            total_amount: total_profit,
            recipient_count,
            timestamp: env.ledger().timestamp(),
            recipients,
        };

        distributions.push_back(record);
        env.storage().instance().set(&DataKey::Distributions, &distributions);

        env.events().publish(
            (Symbol::new(&env, "dividend_paid"),),
            (distribution_id, period),
        );
    }

    /// Get all distribution records.
    ///
    /// # Authorization
    ///
    /// None — read-only query.
    ///
    /// # Panics
    ///
    /// Never panics — returns an empty `Vec` if no distributions exist.
    ///
    /// # Returns
    ///
    /// A `Vec<Distribution>` of all past distributions.
    pub fn get_distributions(env: Env) -> Vec<Distribution> {
        env.storage().instance()
            .get(&DataKey::Distributions)
            .unwrap_or(Vec::new(&env))
    }

    /// Assert that `caller` is the stored admin, panicking otherwise.
    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}
