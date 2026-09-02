//! Voting contract for Soroban-based cooperative governance.
//!
//! Manages on-chain governance proposals for the cooperative, including
//! creation, voting, and finalization. Members vote directly on proposals;
//! proposals pass when they reach a quorum and have more votes in favor
//! than against.
//!
//! The contract is `no_std` and Soroban-targeted.
//!
//! # Events
//!
//! - `proposal_created` — emitted when a new proposal is registered.
//! - `vote_cast` — emitted when a member casts a vote.
//! - `proposal_finalized` — emitted when a proposal's status is finalized.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, String};

/// Storage keys for the voting contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address (set at initialization).
    Admin,
    /// Address of the treasury contract this voting module is tied to.
    TreasuryContract,
    /// Persistent vector of all proposals.
    Proposals,
    /// Monotonically increasing proposal counter.
    ProposalCounter,
    /// Vote map for a given proposal ID: `Address -> bool` (true = approve).
    Votes(u32),
}

/// Lifecycle status of a governance proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    /// Proposal is open for voting.
    Active,
    /// Proposal reached quorum and more votes in favor than against.
    Passed,
    /// Proposal failed to reach quorum or majority.
    Failed,
    /// Proposal has been executed.
    Executed,
}

/// The kind of action a proposal represents.
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
    /// Change a group rule (interest rate, contrib amount, etc.).
    UpdateRule,
    /// General governance proposal.
    General,
}

/// A single governance proposal.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    /// Unique proposal identifier.
    pub id: u32,
    /// The member who created the proposal.
    pub proposer: Address,
    /// The category of this proposal.
    pub proposal_type: ProposalType,
    /// Short title of the proposal.
    pub title: String,
    /// Human-readable description of the proposal.
    pub description: String,
    /// Number of votes cast in favor.
    pub votes_for: u32,
    /// Number of votes cast against.
    pub votes_against: u32,
    /// Minimum number of votes required for the proposal to be valid.
    pub quorum: u32,
    /// Ledger timestamp deadline for voting.
    pub deadline: u64,
    /// Current status of the proposal.
    pub status: ProposalStatus,
    /// Ledger timestamp when the proposal was created.
    pub created_at: u64,
    /// JSON-encoded action payload for the proposal.
    pub payload: String,
}

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
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

    pub fn get_proposals(env: Env) -> Vec<Proposal> {
        env.storage().instance()
            .get(&DataKey::Proposals)
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_votes(env: Env, proposal_id: u32) -> Map<Address, bool> {
        env.storage().persistent()
            .get(&DataKey::Votes(proposal_id))
            .unwrap_or(Map::new(&env))
    }

    fn find_proposal_idx(proposals: &Vec<Proposal>, id: u32) -> u32 {
        for i in 0..proposals.len() {
            if proposals.get(i).unwrap().id == id { return i; }
        }
        panic!("proposal not found");
    }
}
