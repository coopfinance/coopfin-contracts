#![no_std]
//! # CoopFin Voting Contract
//!
//! On-chain governance voting for the cooperative. Members create proposals
//! and other members vote to approve or reject them. Proposals can cover
//! loan approval, treasury spending, membership changes, and rule updates.
//!
//! ## Proposal Types
//!
//! - `LoanApproval` — approve a member loan
//! - `TreasurySpend` — authorize a treasury withdrawal
//! - `AddMember` / `RemoveMember` — membership management
//! - `UpdateRule` — change a group rule
//! - `General` — catch-all governance proposal

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, String,
};

/// Storage keys for the voting contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address authorized to initialize the contract.
    Admin,
    /// Address of the treasury contract (referenced by proposals).
    TreasuryContract,
    /// List of all proposals.
    Proposals,
    /// Auto-incrementing proposal ID counter.
    ProposalCounter,
    /// Vote map for a specific proposal (`proposal_id -> Map<Address, bool>`).
    Votes(u32),
}

/// The lifecycle status of a proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    /// Open for voting.
    Active,
    /// Approved by majority vote with quorum met.
    Passed,
    /// Rejected or quorum not met.
    Failed,
    /// The proposal's payload has been executed on-chain.
    Executed,
}

/// The type of governance action being proposed.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalType {
    /// Approve a member loan.
    LoanApproval,
    /// Authorize a treasury withdrawal.
    TreasurySpend,
    /// Add a new member to the cooperative.
    AddMember,
    /// Remove a member from the cooperative.
    RemoveMember,
    /// Change a group rule (interest rate, contribution amount, etc.).
    UpdateRule,
    /// General governance proposal.
    General,
}

/// A single governance proposal.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    /// Unique proposal ID (auto-incremented).
    pub id: u32,
    /// The Stellar address that created the proposal.
    pub proposer: Address,
    /// Category of the proposal.
    pub proposal_type: ProposalType,
    /// Short title for display.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Number of "for" votes cast.
    pub votes_for: u32,
    /// Number of "against" votes cast.
    pub votes_against: u32,
    /// Minimum total votes required for the proposal to pass.
    pub quorum: u32,
    /// Ledger timestamp after which voting is closed.
    pub deadline: u64,
    /// Current status of the proposal.
    pub status: ProposalStatus,
    /// Ledger timestamp when the proposal was created.
    pub created_at: u64,
    /// JSON-encoded action payload (interpreted based on `proposal_type`).
    pub payload: String,
}

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    /// Initialize the voting contract.
    ///
    /// Stores the admin and treasury addresses, and initializes the proposal
    /// counter and list.
    ///
    /// # Authorization
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Panics
    ///
    /// No explicit panics — callable only once per deploy.
    ///
    /// # Events
    ///
    /// No events emitted.
    pub fn initialize(env: Env, admin: Address, treasury: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::ProposalCounter, &0u32);
        env.storage().instance().set(&DataKey::Proposals, &Vec::<Proposal>::new(&env));
    }

    /// Create a new governance proposal.
    ///
    /// Increments the proposal counter, computes a deadline based on
    /// `voting_days`, and stores the proposal in `Active` status.
    ///
    /// # Authorization
    ///
    /// Requires `proposer.require_auth()`.
    ///
    /// # Panics
    ///
    /// No explicit panics — but invalid payloads may cause downstream
    /// issues during execution.
    ///
    /// # Events
    ///
    /// Emits `proposal_created` with `(id, proposer)`.
    ///
    /// # Returns
    ///
    /// The new proposal's ID as `u32`.
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

    /// Cast a vote on an active proposal.
    ///
    /// Each member may only vote once per proposal. Vote data is stored
    /// persistently keyed by `(proposal_id, voter_address)`.
    ///
    /// # Authorization
    ///
    /// Requires `voter.require_auth()`.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The proposal is not in `Active` status
    /// - The voting deadline has passed
    /// - The voter has already voted on this proposal
    ///
    /// # Events
    ///
    /// Emits `vote_cast` with `(proposal_id, voter, approve)`.
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

    /// Finalize a proposal after its voting deadline has passed.
    ///
    /// Computes the final outcome: if `total_votes >= quorum` and
    /// `votes_for > votes_against`, the proposal status becomes `Passed`;
    /// otherwise it becomes `Failed`.
    ///
    /// # Authorization
    ///
    /// None — anyone can call once the deadline has passed.
    ///
    /// # Panics
    ///
    /// Panics if the proposal is not in `Active` status or if the
    /// deadline has not yet been reached.
    ///
    /// # Events
    ///
    /// Emits `proposal_finalized` with `(proposal_id, status)`.
    ///
    /// # Returns
    ///
    /// The final [`ProposalStatus`].
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

    /// Get all proposals.
    ///
    /// # Authorization
    ///
    /// None — read-only query.
    ///
    /// # Panics
    ///
    /// Never panics — returns an empty `Vec` if no proposals exist.
    ///
    /// # Returns
    ///
    /// A `Vec<Proposal>` of all proposals.
    pub fn get_proposals(env: Env) -> Vec<Proposal> {
        env.storage().instance()
            .get(&DataKey::Proposals)
            .unwrap_or(Vec::new(&env))
    }

    /// Get the vote map for a specific proposal.
    ///
    /// # Authorization
    ///
    /// None — read-only query.
    ///
    /// # Panics
    ///
    /// Never panics — returns an empty `Map` if no votes have been cast.
    ///
    /// # Returns
    ///
    /// A `Map<Address, bool>` where `true` means "for" and `false` means "against".
    pub fn get_votes(env: Env, proposal_id: u32) -> Map<Address, bool> {
        env.storage().persistent()
            .get(&DataKey::Votes(proposal_id))
            .unwrap_or(Map::new(&env))
    }

    /// Find the index of a proposal by ID, panicking if not found.
    fn find_proposal_idx(proposals: &Vec<Proposal>, id: u32) -> u32 {
        for i in 0..proposals.len() {
            if proposals.get(i).unwrap().id == id { return i; }
        }
        panic!("proposal not found");
    }
}
