#![no_std]

//! Profit distribution contract for a cooperative.
//!
//! [`DividendContract`] accepts an admin-defined profit pool and a list of
//! member share weights, then transfers token payouts proportionally from
//! the contract's own balance to each recipient. Every payout is recorded
//! on-chain as a [`Distribution`] so that the treasury can later audit
//! who received what and when.
//!
//! The contract assumes the asset address is a SAC22 token whose client
//! implements the standard [`token::Client`] `transfer` interface.

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec,
};

/// Storage keys for [`DividendContract`].
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address authorized to call [`DividendContract::distribute`].
    Admin,
    /// Address of the SAC22 token used for payouts.
    AssetAddress,
    /// Address of the sibling treasury contract (informational; not enforced).
    TreasuryContract,
    /// Append-only log of every [`Distribution`] executed by this contract.
    Distributions,
    /// Monotonically increasing counter used to assign distribution IDs.
    DistributionCounter,
}

/// Single on-chain record of one profit-distribution event.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Distribution {
    /// Auto-incremented ID assigned at execution time.
    pub id: u32,
    /// Total profit (in token base units) that was available to distribute.
    pub total_profit: i128,
    /// Sum of all `shares` in the call that produced this distribution.
    pub total_shares: i128,
    /// Recipients in the same order as the call's `recipients` argument.
    pub recipients: Vec<Address>,
    /// Payout actually transferred to each recipient (rounded down).
    pub amounts: Vec<i128>,
    /// Ledger timestamp at which the distribution was executed.
    pub executed_at: u64,
    /// Human-readable period label (e.g. `"Q3-2026"`) supplied by the admin.
    pub period: String,
}

#[contract]
pub struct DividendContract;

#[contractimpl]
impl DividendContract {
    /// Initialize the dividend contract.
    ///
    /// Stores the admin, asset address, treasury contract address, and an
    /// empty distribution counter + log. Must be called exactly once.
    ///
    /// # Authorization
    /// Requires auth from `admin`.
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

    /// Return the full on-chain history of [`Distribution`] events, oldest first.
    ///
    /// Returns an empty vector if no distributions have been executed yet.
    pub fn get_distributions(env: Env) -> Vec<Distribution> {
        env.storage().instance()
            .get(&DataKey::Distributions)
            .unwrap_or(Vec::new(&env))
    }

    /// Internal: assert that `caller` is the admin registered at [`initialize`].
    ///
    /// # Panics
    /// Panics with `"unauthorized"` when `caller` does not match.
    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != *caller { panic!("unauthorized"); }
    }
}
