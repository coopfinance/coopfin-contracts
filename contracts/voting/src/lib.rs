#![no_std]

//! On-chain governance: proposal + simple yes/no voting for a cooperative.
//!
//! [`VotingContract`] owns the lifecycle of every [`Proposal`]: members
//! call [`create_proposal`] to open one, then [`vote`] to cast their
//! ballot, and finally [`finalize`] to close the proposal once the
//! deadline has passed.
//!
//! Pass / fail criteria are intentionally simple: a proposal passes iff
//! `votes_for > votes_against` AND total votes reach the configured quorum.
//! There is no delegate voting and no vote replacement; one address, one
//! vote per proposal.

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, String,
};

/// Storage keys for [`VotingContract`].
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address (informational; not enforced in current code).
    Admin,
    /// Address of the sibling treasury contract.
    TreasuryContract,
    /// Append-only log of every [`Proposal`] created on this contract.
    Proposals,
    /// Monotonically increasing counter used to assign proposal IDs.
    ProposalCounter,
    /// Persistent storage: per-proposal map of `voter -> approve`.
    Votes(u32),
}

/// Lifecycle status of a [`Proposal`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    /// Open for voting; [`vote`] is allowed.
    Active,
    /// Deadline passed and quorum + majority reached.
    Passed,
    /// Deadline passed and quorum / majority not reached.
    Failed,
    /// Passed proposal whose payload has been executed (terminal state).
    Executed,
}

/// Coarse categorization of a proposal; useful for off-chain UIs.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalType {
    /// Approve a member loan.
    LoanApproval,
    /// Authorize a treasury withdrawal.
    TreasurySpend,
    /// Add a new member to the coop.
    AddMember,
    /// Remove a member from the coop.
    RemoveMember,
    /// Change a group rule (interest rate, contribution amount, etc.).
    UpdateRule,
    /// General governance proposal.
    General,
}

/// Single proposal record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    /// Auto-incremented ID assigned at create time.
    pub id: u32,
    /// Address that opened the proposal.
    pub proposer: Address,
    /// Coarse proposal category.
    pub proposal_type: ProposalType,
    /// Short title (max 64 chars enforced off-chain).
    pub title: String,
    /// Long-form description (markdown encouraged off-chain).
    pub description: String,
    /// Running total of yes votes.
    pub votes_for: u32,
    /// Running total of no votes.
    pub votes_against: u32,
    /// Minimum total votes required for the proposal to pass.
    pub quorum: u32,
    /// Ledger timestamp after which voting is closed.
    pub deadline: u64,
    /// Current status; see [`ProposalStatus`].
    pub status: ProposalStatus,
    /// Ledger timestamp at creation.
    pub created_at: u64,
    /// JSON-encoded action payload consumed by an executor contract.
    pub payload: String,
}

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    /// Initialize the voting contract.
    ///
    /// Stores the admin, treasury contract address, an empty proposal
    /// counter, and an empty proposals log. Must be called exactly once.
    ///
    /// # Authorization
    /// Requires auth from `admin`.
    pub fn initialize(env: Env, admin: Address, treasury: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::ProposalCounter, &0u32);
        env.storage().instance().set(&DataKey::Proposals, &Vec::<Proposal>::new(&env));
    }

    /// Create a new governance proposal.
    pub fn create_proposal(
        env: Env,
        proposer: Address,
        proposal_type: ProposalType,
        title: String,
        description: String,
        voting_days: u32,
        quorum: u32,
        payload: String,
    ) -> u32 {
        proposer.require_auth();

        let counter: u32 = env.storage().instance()
            .get(&DataKey::ProposalCounter).unwrap_or(0);
        let id = counter + 1;

        let seconds_per_day: u64 = 86_400;
        let deadline = env.ledger().timestamp() + (voting_days as u64 * seconds_per_day);

        let proposal = Proposal {
            id,
            proposer: proposer.clone(),
            proposal_type,
            title,
            description,
            votes_for: 0,
            votes_against: 0,
            quorum,
            deadline,
            status: ProposalStatus::Active,
            created_at: env.ledger().timestamp(),
            payload,
        };

        let mut proposals: Vec<Proposal> = env.storage().instance()
            .get(&DataKey::Proposals).unwrap_or(Vec::new(&env));
        proposals.push_back(proposal);
        env.storage().instance().set(&DataKey::Proposals, &proposals);
        env.storage().instance().set(&DataKey::ProposalCounter, &id);

        // Initialize empty vote map for this proposal
        env.storage().persistent()
            .set(&DataKey::Votes(id), &Map::<Address, bool>::new(&env));

        env.events().publish(
            (Symbol::new(&env, "proposal_created"),),
            (id, proposer),
        );
        id
    }

    /// Member casts a vote on a proposal.
    pub fn vote(env: Env, voter: Address, proposal_id: u32, approve: bool) {
        voter.require_auth();

        let mut proposals: Vec<Proposal> = env.storage().instance()
            .get(&DataKey::Proposals).unwrap();
        let idx = Self::find_proposal_idx(&proposals, proposal_id);
        let mut proposal = proposals.get(idx).unwrap();

        if proposal.status != ProposalStatus::Active {
            panic!("proposal is not active");
        }
        if env.ledger().timestamp() > proposal.deadline {
            panic!("voting period ended");
        }

        let mut votes: Map<Address, bool> = env.storage().persistent()
            .get(&DataKey::Votes(proposal_id))
            .unwrap_or(Map::new(&env));

        if votes.contains_key(voter.clone()) {
            panic!("already voted");
        }

        votes.set(voter.clone(), approve);
        env.storage().persistent().set(&DataKey::Votes(proposal_id), &votes);

        if approve {
            proposal.votes_for += 1;
        } else {
            proposal.votes_against += 1;
        }

        proposals.set(idx, proposal.clone());
        env.storage().instance().set(&DataKey::Proposals, &proposals);

        env.events().publish(
            (Symbol::new(&env, "vote_cast"),),
            (proposal_id, voter, approve),
        );
    }

    /// Finalize a proposal after deadline.
    pub fn finalize(env: Env, proposal_id: u32) -> ProposalStatus {
        let mut proposals: Vec<Proposal> = env.storage().instance()
            .get(&DataKey::Proposals).unwrap();
        let idx = Self::find_proposal_idx(&proposals, proposal_id);
        let mut proposal = proposals.get(idx).unwrap();

        if proposal.status != ProposalStatus::Active {
            panic!("already finalized");
        }
        if env.ledger().timestamp() <= proposal.deadline {
            panic!("voting still active");
        }

        let total_votes = proposal.votes_for + proposal.votes_against;
        proposal.status = if total_votes >= proposal.quorum
            && proposal.votes_for > proposal.votes_against
        {
            ProposalStatus::Passed
        } else {
            ProposalStatus::Failed
        };

        let status = proposal.status.clone();
        proposals.set(idx, proposal);
        env.storage().instance().set(&DataKey::Proposals, &proposals);

        env.events().publish(
            (Symbol::new(&env, "proposal_finalized"),),
            (proposal_id, status.clone()),
        );
        status
    }

    /// Return every proposal ever created on this contract, oldest first.
///
/// Returns an empty vector if no proposals have been created yet.
    pub fn get_proposals(env: Env) -> Vec<Proposal> {
        env.storage().instance()
            .get(&DataKey::Proposals)
            .unwrap_or(Vec::new(&env))
    }

    /// Return the full `voter -> approve` map for `proposal_id`.
    ///
    /// Returns an empty map if no votes have been cast.
    pub fn get_votes(env: Env, proposal_id: u32) -> Map<Address, bool> {
        env.storage().persistent()
            .get(&DataKey::Votes(proposal_id))
            .unwrap_or(Map::new(&env))
    }

    /// Internal: linear search for a proposal by ID in the on-chain log.
    ///
    /// # Panics
    /// Panics with `"proposal not found"` when no proposal has the
    /// supplied ID.
    fn find_proposal_idx(proposals: &Vec<Proposal>, id: u32) -> u32 {
        for i in 0..proposals.len() {
            if proposals.get(i).unwrap().id == id { return i; }
        }
        panic!("proposal not found");
    }
}
