#![no_std]
//! Governance proposal and voting contract for CoopFinance groups.
//!
//! Stores proposals, records one vote per voter per proposal, finalizes
//! proposal outcomes after their deadlines, and exposes proposal/vote views.

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, String,
};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TreasuryContract,
    Proposals,
    ProposalCounter,
    Votes(u32), // proposal_id -> Map<Address, bool>
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    Active,
    Passed,
    Failed,
    Executed,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalType {
    LoanApproval,    // Approve a member loan
    TreasurySpend,   // Authorize a treasury withdrawal
    AddMember,       // Add a new member to the coop
    RemoveMember,    // Remove a member from the coop
    UpdateRule,      // Change a group rule (interest rate, contrib amount, etc.)
    General,         // General governance proposal
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub title: String,
    pub description: String,
    pub votes_for: u32,
    pub votes_against: u32,
    pub quorum: u32,          // Minimum votes required
    pub deadline: u64,        // Ledger timestamp
    pub status: ProposalStatus,
    pub created_at: u64,
    pub payload: String,      // JSON-encoded action payload
}

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    /// Initializes proposal storage and links the treasury contract.
    ///
    /// # Authorization
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    /// Does not explicitly guard against reinitialization.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns nothing.
    pub fn initialize(env: Env, admin: Address, treasury: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::ProposalCounter, &0u32);
        env.storage().instance().set(&DataKey::Proposals, &Vec::<Proposal>::new(&env));
    }

    /// Creates a new governance proposal with a deadline and quorum.
    ///
    /// # Authorization
    /// Requires authorization from `proposer`.
    ///
    /// # Panics
    /// Does not explicitly panic for valid Soroban storage operations.
    ///
    /// # Events
    /// Emits `proposal_created` with `(proposal_id, proposer)`.
    ///
    /// # Returns
    /// Returns the newly assigned proposal id.
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

    /// Casts a yes/no vote for an active proposal.
    ///
    /// # Authorization
    /// Requires authorization from `voter`.
    ///
    /// # Panics
    /// Panics if the proposal does not exist, is not active, the deadline has
    /// passed, or the voter has already voted.
    ///
    /// # Events
    /// Emits `vote_cast` with `(proposal_id, voter, approve)`.
    ///
    /// # Returns
    /// Returns nothing.
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

    /// Finalizes an active proposal after its voting deadline.
    ///
    /// # Authorization
    /// No authorization is required.
    ///
    /// # Panics
    /// Panics if the proposal does not exist, is already finalized, or the
    /// voting deadline has not passed.
    ///
    /// # Events
    /// Emits `proposal_finalized` with `(proposal_id, status)`.
    ///
    /// # Returns
    /// Returns the proposal's final status.
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

    /// Returns all proposals stored by the voting contract.
    ///
    /// # Authorization
    /// No authorization is required.
    ///
    /// # Panics
    /// Does not panic.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns a vector of proposals, or an empty vector if none exist.
    pub fn get_proposals(env: Env) -> Vec<Proposal> {
        env.storage().instance()
            .get(&DataKey::Proposals)
            .unwrap_or(Vec::new(&env))
    }

    /// Returns the vote map for a proposal.
    ///
    /// # Authorization
    /// No authorization is required.
    ///
    /// # Panics
    /// Does not panic.
    ///
    /// # Events
    /// Emits no events.
    ///
    /// # Returns
    /// Returns a map from voter address to approval choice, or an empty map if
    /// no votes are recorded.
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
