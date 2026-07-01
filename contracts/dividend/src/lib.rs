//! # Dividend Contract
//!
//! Distributes cooperative profits to members proportionally based on their
//! share weight. This contract holds no funds itself; it pulls the
//! configured token from its own balance (pre-funded by the treasury) and
//! transfers each member's calculated payout.
//!
//! ## Overview
//!
//! - The **admin** calls [`DividendContract::distribute`] with a list of
//!   recipients, their respective share weights, the total profit amount,
//!   and a period label.
//! - Each member receives `profit × (member_shares / total_shares)`.
//! - Distribution records are stored for auditability and can be queried
//!   via [`DividendContract::get_distributions`].
//!
//! ## Storage
//!
//! Distribution records are stored in instance storage as a `Vec<Distribution>`.
//! A monotonically increasing counter tracks distribution IDs.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec, String};

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
    /// Initialize the dividend contract with an admin, asset token, and
    /// treasury reference.
    ///
    /// Must be called exactly once before any distribution. Sets the
    /// initial distribution counter to zero.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    ///
    /// None on first call (no duplicate-init guard).
    ///
    /// # Events
    ///
    /// None emitted directly by this function.
    ///
    /// # Return value
    ///
    /// Returns `()`.
    pub fn initialize(env: Env, admin: Address, asset: Address, treasury: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::AssetAddress, &asset);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::DistributionCounter, &0u32);
        env.storage().instance()
            .set(&DataKey::Distributions, &Vec::<Distribution>::new(&env));
    }

    /// Distribute profit proportionally based on each member's share weight.
    ///
    /// `recipients` and `shares` must be equal length.
    /// Each member receives: `profit * (member_shares / total_shares)`
    ///
    /// Transfers the calculated payout from this contract to each recipient
    /// and records the distribution for future auditing.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`. Caller must also be the
    /// registered admin of this contract.
    ///
    /// # Panics
    ///
    /// - If the caller is not the admin (`"unauthorized"`).
    /// - If `recipients` and `shares` have different lengths
    ///   (`"recipients and shares length mismatch"`).
    /// - If `total_profit` is less than or equal to zero
    ///   (`"profit must be positive"`).
    /// - If the sum of all shares is zero (`"total shares cannot be zero"`).
    /// - If any individual token transfer fails (insufficient balance in
    ///   contract).
    ///
    /// # Events
    ///
    /// - `"dividend_distributed"` — emitted with `(id, total_profit,
    ///   recipient_count)`.
    ///
    /// # Return value
    ///
    /// Returns `u32` — the newly assigned distribution ID.
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

    /// Get all past distributions.
    ///
    /// Returns the full list of distribution records in chronological order.
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
    /// Returns `Vec<Distribution>` — all distribution records, or an empty
    /// vector if no distributions have been executed.
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
