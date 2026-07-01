//! # Voting Contract
//!
//! On-chain governance voting for cooperative decisions. Members create
//! proposals and vote on them. After the voting deadline, any caller can
//! finalize a proposal to determine whether it passed or failed based on
//! quorum and majority rules.
//!
//! ## Overview
//!
//! - **Any member** can create a proposal via [`VotingContract::create_proposal`]
//!   specifying type, title, description, voting period, quorum, and an
//!   action payload.
//! - **Members** vote on active proposals via [`VotingContract::vote`].
//!   Each member may vote exactly once per proposal.
//! - **Anyone** can call [`VotingContract::finalize`] after the deadline to
//!   tally votes and transition the proposal to `Passed` or `Failed`.
//!
//! Proposal types include loan approvals, treasury spending, membership
//! changes, rule updates, and general governance motions.
//!
//! ## Storage
//!
//! Proposals are stored in instance storage. Per-proposal vote maps are
//! stored in persistent storage keyed by proposal ID to survive contract
//! upgrades.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, String};

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
    /// Initialize the voting contract with an admin and treasury reference.
    ///
    /// Must be called exactly once. Sets the initial proposal counter to zero
    /// and stores the admin and treasury addresses used for authorization
    /// and action execution.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `admin`.
    ///
    /// # Panics
    ///
    /// None on first call. May panic if called a second time (overwrites
    /// storage without guard — consider adding an initialization check).
    ///
    /// # Events
    ///
    /// None emitted directly by this function.
    ///
    /// # Return value
    ///
    /// Returns `()`.
    pub fn initialize(env: Env, admin: Address, treasury: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TreasuryContract, &treasury);
        env.storage().instance().set(&DataKey::ProposalCounter, &0u32);
        env.storage().instance().set(&DataKey::Proposals, &Vec::<Proposal>::new(&env));
    }

    /// Create a new governance proposal.
    ///
    /// Creates a proposal in `Active` status with a voting deadline
    /// calculated from `voting_days`. An empty vote map is initialized
    /// for the new proposal. The returned ID can be used to vote on or
    /// finalize the proposal.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `proposer`.
    ///
    /// # Panics
    ///
    /// None under normal conditions (assumes caller has auth).
    ///
    /// # Events
    ///
    /// - `"proposal_created"` — emitted with `(id, proposer)`.
    ///
    /// # Return value
    ///
    /// Returns `u32` — the newly assigned proposal ID.
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
    ///
    /// Records the voter's choice (`approve = true` for yes, `false` for
    /// no) and increments the corresponding vote counter. Each address may
    /// vote exactly once per proposal.
    ///
    /// # Authorization
    ///
    /// Requires authorization from `voter`.
    ///
    /// # Panics
    ///
    /// - If the proposal is not in `Active` status (`"proposal is not active"`).
    /// - If the voting deadline has passed (`"voting period ended"`).
    /// - If the voter has already voted on this proposal (`"already voted"`).
    /// - If the proposal ID does not exist (`"proposal not found"`).
    ///
    /// # Events
    ///
    /// - `"vote_cast"` — emitted with `(proposal_id, voter, approve)`.
    ///
    /// # Return value
    ///
    /// Returns `()`.
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
    ///
    /// Tallies the votes and determines the outcome: the proposal passes
    /// if the total vote count meets the quorum and the "for" votes exceed
    /// the "against" votes. Otherwise it fails.
    ///
    /// # Authorization
    ///
    /// None required — anyone can finalize after the deadline.
    ///
    /// # Panics
    ///
    /// - If the proposal is not in `Active` status (`"already finalized"`).
    /// - If the voting deadline has not yet passed (`"voting still active"`).
    /// - If the proposal ID does not exist (`"proposal not found"`).
    ///
    /// # Events
    ///
    /// - `"proposal_finalized"` — emitted with `(proposal_id, status)`.
    ///
    /// # Return value
    ///
    /// Returns [`ProposalStatus`] — either `Passed` or `Failed`.
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
    /// Returns the full list of proposals ever created, regardless of
    /// status.
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
    /// Returns `Vec<Proposal>` — all proposal records, or an empty vector
    /// if none have been created.
    pub fn get_proposals(env: Env) -> Vec<Proposal> {
        env.storage().instance()
            .get(&DataKey::Proposals)
            .unwrap_or(Vec::new(&env))
    }

    /// Get the vote map for a specific proposal.
    ///
    /// Returns a mapping of voter addresses to their vote choice (`true`
    /// = approve, `false` = reject).
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
    /// Returns `Map<Address, bool>` — the vote map for the proposal, or
    /// an empty map if no votes have been cast.
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
