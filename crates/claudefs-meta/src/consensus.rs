//! Raft consensus implementation for the metadata service.
//!
//! This module implements a single Raft group state machine for managing
//! distributed metadata operations with strong consistency guarantees.

use std::collections::{HashMap, HashSet};

use crate::types::*;

/// Configuration for a Raft node.
pub struct RaftConfig {
    /// This node's unique identifier.
    pub node_id: NodeId,
    /// Other nodes in the Raft group (excluding this node).
    pub peers: Vec<NodeId>,
    /// Minimum election timeout in milliseconds (default: 150).
    pub election_timeout_min_ms: u64,
    /// Maximum election timeout in milliseconds (default: 300).
    pub election_timeout_max_ms: u64,
    /// Heartbeat interval in milliseconds (default: 50).
    pub heartbeat_interval_ms: u64,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            node_id: NodeId::new(0),
            peers: Vec::new(),
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
        }
    }
}

/// A Raft node implementing the consensus state machine.
pub struct RaftNode {
    config: RaftConfig,
    state: RaftState,
    current_term: Term,
    voted_for: Option<NodeId>,
    log: Vec<LogEntry>,
    commit_index: LogIndex,
    last_applied: LogIndex,
    next_index: HashMap<NodeId, LogIndex>,
    match_index: HashMap<NodeId, LogIndex>,
    votes_received: HashSet<NodeId>,
}

impl RaftNode {
    /// Create a new Raft node starting as a Follower.
    pub fn new(config: RaftConfig) -> Self {
        tracing::debug!(
            node_id = %config.node_id,
            peers = ?config.peers,
            "creating new Raft node as Follower"
        );
        Self {
            config,
            state: RaftState::Follower,
            current_term: Term::new(0),
            voted_for: None,
            log: Vec::new(),
            commit_index: LogIndex::ZERO,
            last_applied: LogIndex::ZERO,
            next_index: HashMap::new(),
            match_index: HashMap::new(),
            votes_received: HashSet::new(),
        }
    }

    /// Get the current term.
    pub fn current_term(&self) -> Term {
        self.current_term
    }

    /// Get the current state.
    pub fn state(&self) -> RaftState {
        self.state
    }

    /// Get the commit index.
    pub fn commit_index(&self) -> LogIndex {
        self.commit_index
    }

    /// Get the last applied index.
    pub fn last_applied(&self) -> LogIndex {
        self.last_applied
    }

    /// Get the node this node voted for in the current term.
    pub fn voted_for(&self) -> Option<NodeId> {
        self.voted_for
    }

    /// Get the log entries from the given index onwards.
    pub fn log_entries_from(&self, from: LogIndex) -> &[LogEntry] {
        let idx = from.as_u64() as usize;
        if idx == 0 {
            &self.log
        } else if idx <= self.log.len() {
            &self.log[idx.saturating_sub(1)..]
        } else {
            &[]
        }
    }

    /// Get a specific log entry by index.
    pub fn log_entry(&self, index: LogIndex) -> Option<&LogEntry> {
        let idx = index.as_u64() as usize;
        if idx == 0 {
            None
        } else if idx <= self.log.len() {
            self.log.get(idx - 1)
        } else {
            None
        }
    }

    /// Get the last log index.
    pub fn last_log_index(&self) -> LogIndex {
        LogIndex::new(self.log.len() as u64)
    }

    /// Get the term of the last log entry.
    pub fn last_log_term(&self) -> Term {
        self.log.last().map(|e| e.term).unwrap_or(Term::new(0))
    }

    /// Start an election: transition to Candidate, increment term, vote for self.
    /// Returns a RequestVote message to broadcast to all peers.
    pub fn start_election(&mut self) -> RaftMessage {
        tracing::info!(
            node_id = %self.config.node_id,
            term = %self.current_term,
            "starting election"
        );

        self.state = RaftState::Candidate;
        self.current_term = Term::new(self.current_term.as_u64() + 1);
        self.voted_for = Some(self.config.node_id);
        self.votes_received.clear();
        self.votes_received.insert(self.config.node_id);

        let last_log_index = self.last_log_index();
        let last_log_term = self.last_log_term();

        tracing::debug!(
            node_id = %self.config.node_id,
            term = %self.current_term,
            last_log_index = %last_log_index,
            last_log_term = %last_log_term,
            "sending RequestVote"
        );

        RaftMessage::RequestVote {
            term: self.current_term,
            candidate_id: self.config.node_id,
            last_log_index,
            last_log_term,
        }
    }

    /// Handle a RequestVote message. Returns a RequestVoteResponse.
    pub fn handle_request_vote(&mut self, msg: &RaftMessage) -> RaftMessage {
        let (term, candidate_id, last_log_index, last_log_term) = match msg {
            RaftMessage::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
            } => (*term, *candidate_id, *last_log_index, *last_log_term),
            _ => {
                panic!("handle_request_vote called with non-RequestVote message");
            }
        };

        tracing::debug!(
            node_id = %self.config.node_id,
            current_term = %self.current_term,
            candidate_term = %term,
            candidate_id = %candidate_id,
            "received RequestVote"
        );

        if term > self.current_term {
            tracing::info!(
                node_id = %self.config.node_id,
                old_term = %self.current_term,
                new_term = %term,
                "stepping down to follower due to higher term"
            );
            self.step_down(term);
        }

        let vote_granted = if term < self.current_term {
            tracing::debug!(
                node_id = %self.config.node_id,
                "rejecting vote: candidate term {} < current term {}",
                term.as_u64(),
                self.current_term.as_u64()
            );
            false
        } else if let Some(voted_for) = self.voted_for {
            if voted_for != candidate_id {
                tracing::debug!(
                    node_id = %self.config.node_id,
                    "rejecting vote: already voted for {}",
                    voted_for
                );
                false
            } else {
                tracing::debug!(
                    node_id = %self.config.node_id,
                    "granting vote: already voted for this candidate"
                );
                true
            }
        } else if !self.is_log_up_to_date(last_log_index, last_log_term) {
            tracing::debug!(
                node_id = %self.config.node_id,
                "rejecting vote: candidate log is not up to date"
            );
            false
        } else {
            tracing::info!(
                node_id = %self.config.node_id,
                candidate_id = %candidate_id,
                "granting vote to candidate"
            );
            self.voted_for = Some(candidate_id);
            true
        };

        RaftMessage::RequestVoteResponse {
            term: self.current_term,
            vote_granted,
        }
    }

    /// Handle a RequestVoteResponse. If we've won the election,
    /// transition to Leader and return the initial empty AppendEntries (heartbeat)
    /// messages to send to all peers. Returns None if election not yet won.
    pub fn handle_vote_response(
        &mut self,
        from: NodeId,
        msg: &RaftMessage,
    ) -> Option<Vec<(NodeId, RaftMessage)>> {
        let (term, vote_granted) = match msg {
            RaftMessage::RequestVoteResponse { term, vote_granted } => (*term, *vote_granted),
            _ => {
                panic!("handle_vote_response called with non-RequestVoteResponse message");
            }
        };

        tracing::debug!(
            node_id = %self.config.node_id,
            from = %from,
            term = %term,
            vote_granted = vote_granted,
            "received vote response"
        );

        if self.state != RaftState::Candidate {
            tracing::debug!(
                node_id = %self.config.node_id,
                "not in Candidate state, ignoring vote response"
            );
            return None;
        }

        if term > self.current_term {
            tracing::info!(
                node_id = %self.config.node_id,
                old_term = %self.current_term,
                new_term = %term,
                "stepping down to follower due to higher term in vote response"
            );
            self.step_down(term);
            return None;
        }

        if vote_granted {
            self.votes_received.insert(from);
        }

        let majority = (self.config.peers.len() + 2) / 2;
        if self.votes_received.len() >= majority {
            tracing::info!(
                node_id = %self.config.node_id,
                votes = %self.votes_received.len(),
                majority = %majority,
                "won election, becoming Leader"
            );

            self.state = RaftState::Leader;
            let last_idx = self.last_log_index();

            for peer in &self.config.peers {
                self.next_index
                    .insert(*peer, LogIndex::new(last_idx.as_u64() + 1));
                self.match_index.insert(*peer, LogIndex::ZERO);
            }

            let mut messages = Vec::new();
            for peer in &self.config.peers {
                messages.push((*peer, self.build_append_entries(*peer)));
            }
            Some(messages)
        } else {
            None
        }
    }

    /// Propose a new operation. Only valid when Leader.
    /// Appends to local log, returns AppendEntries messages for followers.
    pub fn propose(&mut self, op: MetaOp) -> Result<Vec<(NodeId, RaftMessage)>, MetaError> {
        if self.state != RaftState::Leader {
            return Err(MetaError::NotLeader { leader_hint: None });
        }

        let index = LogIndex::new(self.log.len() as u64 + 1);
        let entry = LogEntry {
            index,
            term: self.current_term,
            op,
        };

        tracing::debug!(
            node_id = %self.config.node_id,
            term = %self.current_term,
            index = %index,
            "proposing new entry"
        );

        self.log.push(entry);

        let mut messages = Vec::new();
        for peer in &self.config.peers {
            messages.push((*peer, self.build_append_entries(*peer)));
        }

        Ok(messages)
    }

    /// Handle an AppendEntries message (as Follower/Candidate).
    /// Returns an AppendEntriesResponse.
    pub fn handle_append_entries(&mut self, msg: &RaftMessage) -> RaftMessage {
        let (term, leader_id, prev_log_index, prev_log_term, entries, leader_commit) = match msg {
            RaftMessage::AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            } => (
                *term,
                *leader_id,
                *prev_log_index,
                *prev_log_term,
                entries.clone(),
                *leader_commit,
            ),
            _ => {
                panic!("handle_append_entries called with non-AppendEntries message");
            }
        };

        tracing::debug!(
            node_id = %self.config.node_id,
            current_term = %self.current_term,
            leader_term = %term,
            leader_id = %leader_id,
            prev_log_index = %prev_log_index,
            leader_commit = %leader_commit,
            "received AppendEntries"
        );

        if term > self.current_term {
            tracing::info!(
                node_id = %self.config.node_id,
                old_term = %self.current_term,
                new_term = %term,
                "stepping down to follower due to higher term in AppendEntries"
            );
            self.step_down(term);
        }

        if term < self.current_term {
            tracing::debug!(
                node_id = %self.config.node_id,
                "rejecting AppendEntries: leader term {} < current term {}",
                term.as_u64(),
                self.current_term.as_u64()
            );
            return RaftMessage::AppendEntriesResponse {
                term: self.current_term,
                success: false,
                match_index: self.last_log_index(),
            };
        }

        if self.state == RaftState::Candidate {
            tracing::info!(
                node_id = %self.config.node_id,
                "becoming follower after receiving AppendEntries from valid leader"
            );
            self.state = RaftState::Follower;
        }

        if prev_log_index.as_u64() > 0 {
            let prev_entry = self.log_entry(prev_log_index);
            let prev_entry_term = prev_entry.map(|e| e.term).unwrap_or(Term::new(0));

            if prev_entry_term != prev_log_term {
                tracing::debug!(
                    node_id = %self.config.node_id,
                    prev_log_index = %prev_log_index,
                    expected_term = %prev_log_term,
                    actual_term = %prev_entry_term,
                    "rejecting AppendEntries: prev_log_term mismatch"
                );

                self.log.truncate(prev_log_index.as_u64() as usize);

                return RaftMessage::AppendEntriesResponse {
                    term: self.current_term,
                    success: false,
                    match_index: self.last_log_index(),
                };
            }
        }

        let existing_len = self.log.len();
        let new_entries_start = prev_log_index.as_u64() as usize;

        for (i, entry) in entries.iter().enumerate() {
            let idx = new_entries_start + i;
            if idx < existing_len {
                if self.log[idx].term != entry.term {
                    tracing::debug!(
                        node_id = %self.config.node_id,
                        idx = %entry.index,
                        "truncating log at index {} due to term mismatch",
                        idx + 1
                    );
                    self.log.truncate(idx);
                    self.log.push(entry.clone());
                }
            } else {
                self.log.push(entry.clone());
            }
        }

        if leader_commit > self.commit_index {
            let new_commit = std::cmp::min(leader_commit, self.last_log_index());
            if new_commit > self.commit_index {
                tracing::debug!(
                    node_id = %self.config.node_id,
                    old_commit = %self.commit_index,
                    new_commit = %new_commit,
                    "advancing commit index"
                );
                self.commit_index = new_commit;
            }
        }

        tracing::debug!(
            node_id = %self.config.node_id,
            log_len = %self.log.len(),
            commit_index = %self.commit_index,
            "AppendEntries accepted"
        );

        RaftMessage::AppendEntriesResponse {
            term: self.current_term,
            success: true,
            match_index: self.last_log_index(),
        }
    }

    /// Handle an AppendEntriesResponse (as Leader).
    /// Updates match_index/next_index, advances commit_index if majority.
    /// Returns newly committed entries (if any).
    pub fn handle_append_response(&mut self, from: NodeId, msg: &RaftMessage) -> Vec<LogEntry> {
        let (term, success, match_index) = match msg {
            RaftMessage::AppendEntriesResponse {
                term,
                success,
                match_index,
            } => (*term, *success, *match_index),
            _ => {
                panic!("handle_append_response called with non-AppendEntriesResponse message");
            }
        };

        tracing::debug!(
            node_id = %self.config.node_id,
            from = %from,
            term = %term,
            success = success,
            match_index = %match_index,
            "received AppendEntriesResponse"
        );

        if self.state != RaftState::Leader {
            tracing::debug!(
                node_id = %self.config.node_id,
                "not leader, ignoring AppendEntriesResponse"
            );
            return Vec::new();
        }

        if term > self.current_term {
            tracing::info!(
                node_id = %self.config.node_id,
                old_term = %self.current_term,
                new_term = %term,
                "stepping down to follower due to higher term in AppendEntriesResponse"
            );
            self.step_down(term);
            return Vec::new();
        }

        if success {
            self.next_index
                .insert(from, LogIndex::new(match_index.as_u64() + 1));
            self.match_index.insert(from, match_index);

            tracing::debug!(
                node_id = %self.config.node_id,
                from = %from,
                next_index = %self.next_index.get(&from).unwrap(),
                match_index = %match_index,
                "follower log matched"
            );
        } else if let Some(next_idx) = self.next_index.get(&from).copied() {
            let new_next = LogIndex::new(next_idx.as_u64().saturating_sub(1));
            self.next_index.insert(from, new_next);

            tracing::debug!(
                node_id = %self.config.node_id,
                from = %from,
                old_next = %next_idx,
                new_next = %new_next,
                "decrementing next_index for follower"
            );
        }

        self.try_advance_commit();

        let start = self.last_applied.as_u64() as usize;
        let end = self.commit_index.as_u64() as usize;

        if start < end && end <= self.log.len() {
            self.log[start..end].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Apply committed but not-yet-applied entries. Returns the entries to apply.
    pub fn take_committed_entries(&mut self) -> Vec<LogEntry> {
        let start = self.last_applied.as_u64() as usize;
        let end = self.commit_index.as_u64() as usize;

        if start < end && end <= self.log.len() {
            let entries: Vec<LogEntry> = self.log[start..end].to_vec();
            self.last_applied = self.commit_index;

            tracing::debug!(
                node_id = %self.config.node_id,
                count = %entries.len(),
                last_applied = %self.last_applied,
                "taking committed entries to apply"
            );

            entries
        } else {
            Vec::new()
        }
    }

    /// Step down to follower for the given term (called when we see a higher term).
    fn step_down(&mut self, term: Term) {
        self.current_term = term;
        self.state = RaftState::Follower;
        self.voted_for = None;
    }

    /// Build an AppendEntries message for a specific peer.
    fn build_append_entries(&self, peer: NodeId) -> RaftMessage {
        let next_idx = self
            .next_index
            .get(&peer)
            .copied()
            .unwrap_or(LogIndex::new(self.log.len() as u64 + 1));

        let prev_log_index = LogIndex::new(next_idx.as_u64().saturating_sub(1));
        let prev_log_term = if prev_log_index.as_u64() > 0 {
            self.log_entry(prev_log_index)
                .map(|e| e.term)
                .unwrap_or(Term::new(0))
        } else {
            Term::new(0)
        };

        let entries: Vec<LogEntry> = if next_idx.as_u64() as usize <= self.log.len() {
            self.log[next_idx.as_u64() as usize - 1..].to_vec()
        } else {
            Vec::new()
        };

        RaftMessage::AppendEntries {
            term: self.current_term,
            leader_id: self.config.node_id,
            prev_log_index,
            prev_log_term,
            entries,
            leader_commit: self.commit_index,
        }
    }

    /// Check if the candidate's log is at least as up-to-date as this node's log.
    fn is_log_up_to_date(&self, last_log_index: LogIndex, last_log_term: Term) -> bool {
        let my_last_term = self.last_log_term();

        if last_log_term > my_last_term {
            true
        } else if last_log_term == my_last_term {
            last_log_index >= self.last_log_index()
        } else {
            false
        }
    }

    /// Try to advance the commit index based on match_index quorum.
    fn try_advance_commit(&mut self) {
        if self.state != RaftState::Leader {
            return;
        }

        let last_idx = self.last_log_index();
        for n in (self.commit_index.as_u64() + 1)..=last_idx.as_u64() {
            let idx = LogIndex::new(n);
            let entry_term = self.log_entry(idx).map(|e| e.term);

            if entry_term != Some(self.current_term) {
                continue;
            }

            let mut replication_count = 1;
            for peer in &self.config.peers {
                if let Some(match_idx) = self.match_index.get(peer) {
                    if *match_idx >= idx {
                        replication_count += 1;
                    }
                }
            }

            let majority = (self.config.peers.len() + 2) / 2;
            if replication_count >= majority {
                tracing::debug!(
                    node_id = %self.config.node_id,
                    index = %idx,
                    replication_count = %replication_count,
                    majority = %majority,
                    "committing log entry"
                );
                self.commit_index = idx;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_three_node_config(node_id: NodeId) -> RaftConfig {
        RaftConfig {
            node_id,
            peers: vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
                .into_iter()
                .filter(|&id| id != node_id)
                .collect(),
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
        }
    }

    #[test]
    fn test_new_node_starts_as_follower() {
        let config = create_three_node_config(NodeId::new(1));
        let node = RaftNode::new(config);

        assert_eq!(node.state(), RaftState::Follower);
        assert_eq!(node.current_term(), Term::new(0));
    }

    #[test]
    fn test_start_election_transitions_to_candidate() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        let msg = node.start_election();

        assert_eq!(node.state(), RaftState::Candidate);
        assert_eq!(node.current_term(), Term::new(1));
        assert_eq!(node.voted_for(), Some(NodeId::new(1)));

        match msg {
            RaftMessage::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
            } => {
                assert_eq!(term, Term::new(1));
                assert_eq!(candidate_id, NodeId::new(1));
                assert_eq!(last_log_index, LogIndex::ZERO);
                assert_eq!(last_log_term, Term::new(0));
            }
            _ => panic!("expected RequestVote message"),
        }
    }

    #[test]
    fn test_request_vote_grants_vote_for_fresh_candidate() {
        let config = create_three_node_config(NodeId::new(2));
        let mut node = RaftNode::new(config);

        let msg = RaftMessage::RequestVote {
            term: Term::new(1),
            candidate_id: NodeId::new(1),
            last_log_index: LogIndex::ZERO,
            last_log_term: Term::new(0),
        };

        let response = node.handle_request_vote(&msg);

        match response {
            RaftMessage::RequestVoteResponse { term, vote_granted } => {
                assert_eq!(term, Term::new(1));
                assert!(vote_granted);
            }
            _ => panic!("expected RequestVoteResponse"),
        }
    }

    #[test]
    fn test_request_vote_rejects_lower_term() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        assert_eq!(node.current_term(), Term::new(1));

        let msg = RaftMessage::RequestVote {
            term: Term::new(0),
            candidate_id: NodeId::new(2),
            last_log_index: LogIndex::ZERO,
            last_log_term: Term::new(0),
        };

        let response = node.handle_request_vote(&msg);

        match response {
            RaftMessage::RequestVoteResponse { vote_granted, .. } => {
                assert!(!vote_granted);
            }
            _ => panic!("expected RequestVoteResponse"),
        }
    }

    #[test]
    fn test_request_vote_rejects_if_already_voted() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        let msg1 = RaftMessage::RequestVote {
            term: Term::new(1),
            candidate_id: NodeId::new(2),
            last_log_index: LogIndex::ZERO,
            last_log_term: Term::new(0),
        };
        node.handle_request_vote(&msg1);
        assert_eq!(node.voted_for(), Some(NodeId::new(2)));

        let msg2 = RaftMessage::RequestVote {
            term: Term::new(1),
            candidate_id: NodeId::new(3),
            last_log_index: LogIndex::ZERO,
            last_log_term: Term::new(0),
        };

        let response = node.handle_request_vote(&msg2);

        match response {
            RaftMessage::RequestVoteResponse { vote_granted, .. } => {
                assert!(!vote_granted);
            }
            _ => panic!("expected RequestVoteResponse"),
        }
    }

    #[test]
    fn test_winning_election_with_majority_votes() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        assert_eq!(node.state(), RaftState::Candidate);

        let response1 = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        let result = node.handle_vote_response(NodeId::new(2), &response1);

        assert!(result.is_some());
        assert_eq!(node.state(), RaftState::Leader);

        let messages = result.unwrap();
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_leader_sends_heartbeats() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        let response1 = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        let result = node.handle_vote_response(NodeId::new(2), &response1);

        assert!(result.is_some());
        let messages = result.unwrap();

        for (_, msg) in &messages {
            match msg {
                RaftMessage::AppendEntries { entries, .. } => {
                    assert!(entries.is_empty());
                }
                _ => panic!("expected AppendEntries"),
            }
        }
    }

    #[test]
    fn test_propose_appends_to_leader_log() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        let response1 = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        node.handle_vote_response(NodeId::new(2), &response1);

        let response2 = RaftMessage::RequestVoteResponse {
            term: Term::new(1),
            vote_granted: true,
        };
        node.handle_vote_response(NodeId::new(3), &response2);

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };

        let result = node.propose(op);
        assert!(result.is_ok());

        let messages = result.unwrap();
        assert!(!messages.is_empty());

        assert_eq!(node.last_log_index(), LogIndex::new(1));
    }

    #[test]
    fn test_append_entries_accepts_entries() {
        let config = create_three_node_config(NodeId::new(2));
        let mut node = RaftNode::new(config);

        let entries = vec![LogEntry {
            index: LogIndex::new(1),
            term: Term::new(1),
            op: MetaOp::CreateInode {
                attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
            },
        }];

        let msg = RaftMessage::AppendEntries {
            term: Term::new(1),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::ZERO,
            prev_log_term: Term::new(0),
            entries,
            leader_commit: LogIndex::new(1),
        };

        let response = node.handle_append_entries(&msg);

        match response {
            RaftMessage::AppendEntriesResponse { success, .. } => {
                assert!(success);
            }
            _ => panic!("expected AppendEntriesResponse"),
        }

        assert_eq!(node.commit_index(), LogIndex::new(1));
    }

    #[test]
    fn test_append_entries_rejects_mismatched_prev_log() {
        let config = create_three_node_config(NodeId::new(2));
        let mut node = RaftNode::new(config);

        node.start_election();
        node.handle_vote_response(
            NodeId::new(1),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );
        node.handle_vote_response(
            NodeId::new(3),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };
        let _ = node.propose(op).unwrap();

        let entries = vec![LogEntry {
            index: LogIndex::new(2),
            term: Term::new(2),
            op: MetaOp::CreateInode {
                attr: InodeAttr::new_file(InodeId::new(2), 0, 0, 0o644, 1),
            },
        }];

        let msg = RaftMessage::AppendEntries {
            term: Term::new(2),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::new(1),
            prev_log_term: Term::new(5),
            entries,
            leader_commit: LogIndex::new(2),
        };

        let response = node.handle_append_entries(&msg);

        match response {
            RaftMessage::AppendEntriesResponse { success, .. } => {
                assert!(!success);
            }
            _ => panic!("expected AppendEntriesResponse"),
        }
    }

    #[test]
    fn test_log_replication_and_commit_advancement() {
        let mut leader_config = create_three_node_config(NodeId::new(1));
        leader_config.peers = vec![NodeId::new(2), NodeId::new(3)];
        let mut leader = RaftNode::new(leader_config);

        leader.start_election();
        let _ = leader.handle_vote_response(
            NodeId::new(2),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );
        let _ = leader.handle_vote_response(
            NodeId::new(3),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };
        let _ = leader.propose(op).unwrap();

        let _ = leader.handle_append_response(
            NodeId::new(2),
            &RaftMessage::AppendEntriesResponse {
                term: Term::new(1),
                success: true,
                match_index: LogIndex::new(1),
            },
        );

        assert_eq!(leader.commit_index(), LogIndex::new(1));

        let _ = leader.handle_append_response(
            NodeId::new(3),
            &RaftMessage::AppendEntriesResponse {
                term: Term::new(1),
                success: true,
                match_index: LogIndex::new(1),
            },
        );

        assert_eq!(leader.commit_index(), LogIndex::new(1));
    }

    #[test]
    fn test_step_down_on_higher_term() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        assert_eq!(node.state(), RaftState::Candidate);
        assert_eq!(node.current_term(), Term::new(1));

        node.step_down(Term::new(5));

        assert_eq!(node.state(), RaftState::Follower);
        assert_eq!(node.current_term(), Term::new(5));
        assert_eq!(node.voted_for(), None);
    }

    #[test]
    fn test_three_node_cluster_election() {
        let config1 = create_three_node_config(NodeId::new(1));
        let mut node1 = RaftNode::new(config1);

        let config2 = create_three_node_config(NodeId::new(2));
        let mut node2 = RaftNode::new(config2);

        let config3 = create_three_node_config(NodeId::new(3));
        let mut node3 = RaftNode::new(config3);

        let vote_req = node1.start_election();

        let vote_resp2 = node2.handle_request_vote(&vote_req);
        let _vote_resp3 = node3.handle_request_vote(&vote_req);

        let result = node1.handle_vote_response(NodeId::new(2), &vote_resp2);

        assert!(result.is_some());
        assert_eq!(node1.state(), RaftState::Leader);
    }

    #[test]
    fn test_three_node_cluster_log_replication() {
        let mut leader_config = create_three_node_config(NodeId::new(1));
        leader_config.peers = vec![NodeId::new(2), NodeId::new(3)];
        let mut leader = RaftNode::new(leader_config);

        let mut follower2_config = create_three_node_config(NodeId::new(2));
        follower2_config.peers = vec![NodeId::new(1), NodeId::new(3)];
        let mut follower2 = RaftNode::new(follower2_config);

        let mut follower3_config = create_three_node_config(NodeId::new(3));
        follower3_config.peers = vec![NodeId::new(1), NodeId::new(2)];
        let mut follower3 = RaftNode::new(follower3_config);

        let _ = leader.start_election();
        let _ = leader.handle_vote_response(
            NodeId::new(2),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );

        let op = MetaOp::CreateEntry {
            parent: InodeId::ROOT_INODE,
            name: "test.txt".to_string(),
            entry: DirEntry {
                name: "test.txt".to_string(),
                ino: InodeId::new(1),
                file_type: FileType::RegularFile,
            },
        };

        let propose_result = leader.propose(op).unwrap();

        let append_msg = &propose_result[0].1;
        let _ = follower2.handle_append_entries(append_msg);
        let append_msg = &propose_result[1].1;
        let _ = follower3.handle_append_entries(append_msg);

        assert_eq!(leader.last_log_index(), LogIndex::new(1));

        let _ = leader.handle_append_response(
            NodeId::new(2),
            &RaftMessage::AppendEntriesResponse {
                term: Term::new(1),
                success: true,
                match_index: LogIndex::new(1),
            },
        );

        assert_eq!(leader.commit_index(), LogIndex::new(1));
    }

    #[test]
    fn test_propose_fails_when_not_leader() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };

        let result = node.propose(op);

        assert!(result.is_err());
        match result {
            Err(MetaError::NotLeader { .. }) => {}
            _ => panic!("expected NotLeader error"),
        }
    }

    #[test]
    fn test_append_entries_updates_voted_for() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        let msg = RaftMessage::RequestVote {
            term: Term::new(1),
            candidate_id: NodeId::new(2),
            last_log_index: LogIndex::ZERO,
            last_log_term: Term::new(0),
        };

        let _ = node.handle_request_vote(&msg);

        assert_eq!(node.voted_for(), Some(NodeId::new(2)));
    }

    #[test]
    fn test_take_committed_entries() {
        let config = create_three_node_config(NodeId::new(1));
        let mut node = RaftNode::new(config);

        node.start_election();
        let _ = node.handle_vote_response(
            NodeId::new(2),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );
        let _ = node.handle_vote_response(
            NodeId::new(3),
            &RaftMessage::RequestVoteResponse {
                term: Term::new(1),
                vote_granted: true,
            },
        );

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };
        let _ = node.propose(op).unwrap();

        for peer in ["2", "3"] {
            let peer_id = NodeId::new(peer.parse().unwrap());
            let _ = node.handle_append_response(
                peer_id,
                &RaftMessage::AppendEntriesResponse {
                    term: Term::new(1),
                    success: true,
                    match_index: LogIndex::new(1),
                },
            );
        }

        let committed = node.take_committed_entries();
        assert_eq!(committed.len(), 1);
    }

    #[test]
    fn test_request_vote_accepts_log_up_to_date_candidate() {
        let config = create_three_node_config(NodeId::new(2));
        let mut node = RaftNode::new(config);

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };

        let msg = RaftMessage::AppendEntries {
            term: Term::new(1),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::ZERO,
            prev_log_term: Term::new(0),
            entries: vec![LogEntry {
                index: LogIndex::new(1),
                term: Term::new(1),
                op,
            }],
            leader_commit: LogIndex::new(1),
        };
        let _ = node.handle_append_entries(&msg);

        let vote_req = RaftMessage::RequestVote {
            term: Term::new(2),
            candidate_id: NodeId::new(3),
            last_log_index: LogIndex::new(1),
            last_log_term: Term::new(1),
        };

        let response = node.handle_request_vote(&vote_req);

        match response {
            RaftMessage::RequestVoteResponse { vote_granted, .. } => {
                assert!(vote_granted);
            }
            _ => panic!("expected RequestVoteResponse"),
        }
    }

    #[test]
    fn test_request_vote_rejects_stale_log() {
        let config = create_three_node_config(NodeId::new(3));
        let mut node = RaftNode::new(config);

        let op = MetaOp::CreateInode {
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };

        let msg = RaftMessage::AppendEntries {
            term: Term::new(2),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::ZERO,
            prev_log_term: Term::new(0),
            entries: vec![LogEntry {
                index: LogIndex::new(1),
                term: Term::new(2),
                op,
            }],
            leader_commit: LogIndex::new(1),
        };
        let _ = node.handle_append_entries(&msg);

        let vote_req = RaftMessage::RequestVote {
            term: Term::new(3),
            candidate_id: NodeId::new(2),
            last_log_index: LogIndex::ZERO,
            last_log_term: Term::new(0),
        };

        let response = node.handle_request_vote(&vote_req);

        match response {
            RaftMessage::RequestVoteResponse { vote_granted, .. } => {
                assert!(!vote_granted);
            }
            _ => panic!("expected RequestVoteResponse"),
        }
    }
}
