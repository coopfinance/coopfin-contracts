#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, String,
};

// ─── TTL Constants ────────────────────────────────────────────────────────────
// Instance storage TTL: bump if below 100 ledgers (~5 days), extend to 10,000 (~500 days).
// Persistent storage TTL: bump if below 100 ledgers, extend to 10,000.
// Proposal vote maps are stored in persistent storage and must outlive voting periods.
const INSTANCE_TTL_THRESHOLD: u32 = 100;
const INSTANCE_TTL_EXTEND_TO: u32 = 10_000;
const PERSISTENT_TTL_THRESHOLD: u32 = 100;
const PERSISTENT_TTL_EXTEND_TO: u32 = 10_000;

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
    /// Extend instance storage TTL to prevent data expiration.
    ///
    /// Called at the start of every state-changing function. Uses threshold=100
    /// and extend_to=10,000 ledgers (~500 days) to keep instance data alive.
    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    pub fn initialize(env: Env, admin: Address, treasury: Address) {
        Self::bump_instance(&env);
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
        Self::bump_instance(&env);
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

        // Extend persistent storage TTL for the vote map.
        // threshold=100 ledgers (~5 days), extend_to=10,000 ledgers (~500 days).
        env.storage().persistent().extend_ttl(
            &DataKey::Votes(id),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );

        env.events().publish(
            (Symbol::new(&env, "proposal_created"),),
            (id, proposer),
        );
        id
    }

    /// Member casts a vote on a proposal.
    pub fn vote(env: Env, voter: Address, proposal_id: u32, approve: bool) {
        Self::bump_instance(&env);
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

        // Extend persistent storage TTL for this proposal's vote map
        // after writing new vote data.
        env.storage().persistent().extend_ttl(
            &DataKey::Votes(proposal_id),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );

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
        Self::bump_instance(&env);

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
