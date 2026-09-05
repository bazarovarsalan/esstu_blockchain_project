use std::collections::BTreeMap;

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

use crate::{
    canonical::CanonicalEncoder,
    crypto::{decode_fixed, public_key_hex, sha256_hex, sign_hex},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub address: String,
    pub label: String,
    pub balance: u64,
    /// Следующий ожидаемый номер исходящей транзакции.
    pub nonce: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub id: String,
    pub sender: String,
    pub recipient: String,
    pub amount: u64,
    pub nonce: u64,
    pub signature: String,
}

impl Transaction {
    pub fn unsigned_canonical_bytes_for(
        sender: &str,
        recipient: &str,
        amount: u64,
        nonce: u64,
    ) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("RRQ/TRANSACTION/UNSIGNED");
        encoder.put_str(sender);
        encoder.put_str(recipient);
        encoder.put_u64(amount);
        encoder.put_u64(nonce);
        encoder.finish()
    }

    pub fn unsigned_canonical_bytes(&self) -> Vec<u8> {
        Self::unsigned_canonical_bytes_for(&self.sender, &self.recipient, self.amount, self.nonce)
    }

    pub fn full_canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("RRQ/TRANSACTION/FULL");
        encoder.put_str(&self.id);
        encoder.put_str(&self.sender);
        encoder.put_str(&self.recipient);
        encoder.put_u64(self.amount);
        encoder.put_u64(self.nonce);
        encoder.put_str(&self.signature);
        encoder.finish()
    }

    pub fn recompute_id(&self) -> String {
        sha256_hex(&self.unsigned_canonical_bytes())
    }

    pub fn new_signed(
        signing_key: &SigningKey,
        recipient: String,
        amount: u64,
        nonce: u64,
    ) -> Self {
        let sender = public_key_hex(signing_key);
        let unsigned = Self::unsigned_canonical_bytes_for(&sender, &recipient, amount, nonce);
        Self {
            id: sha256_hex(&unsigned),
            sender,
            recipient,
            amount,
            nonce,
            signature: sign_hex(signing_key, &unsigned),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHeader {
    pub height: u64,
    pub previous_hash: String,
    pub transactions_root: String,
    pub state_hash: String,
    pub timestamp: u64,
    pub round: u64,
    pub proposer: String,
}

impl BlockHeader {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("RRQ/BLOCK/HEADER");
        encoder.put_u64(self.height);
        encoder.put_str(&self.previous_hash);
        encoder.put_str(&self.transactions_root);
        encoder.put_str(&self.state_hash);
        encoder.put_u64(self.timestamp);
        encoder.put_u64(self.round);
        encoder.put_str(&self.proposer);
        encoder.finish()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Vote {
    pub validator_id: String,
    pub block_hash: String,
    pub round: u64,
    pub approve: bool,
    pub signature: String,
}

impl Vote {
    pub fn canonical_message_for(
        validator_id: &str,
        block_hash: &str,
        round: u64,
        approve: bool,
    ) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("RRQ/VOTE");
        encoder.put_str(validator_id);
        let hash = decode_fixed::<32>(block_hash).unwrap_or([0; 32]);
        encoder.put_fixed(&hash);
        encoder.put_u64(round);
        encoder.put_bool(approve);
        encoder.finish()
    }

    pub fn canonical_message(&self) -> Vec<u8> {
        Self::canonical_message_for(
            &self.validator_id,
            &self.block_hash,
            self.round,
            self.approve,
        )
    }

    pub fn signed(
        validator_id: String,
        block_hash: String,
        round: u64,
        signing_key: &SigningKey,
    ) -> Self {
        let message = Self::canonical_message_for(&validator_id, &block_hash, round, true);
        Self {
            validator_id,
            block_hash,
            round,
            approve: true,
            signature: sign_hex(signing_key, &message),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub hash: String,
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub proposer_signature: String,
    pub votes: Vec<Vote>,
}

impl Block {
    pub fn content_canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("RRQ/BLOCK/CONTENT");
        encoder.put_bytes(&self.header.canonical_bytes());
        encoder.put_u32(
            u32::try_from(self.transactions.len()).expect("too many transactions in block"),
        );
        for transaction in &self.transactions {
            encoder.put_bytes(&transaction.full_canonical_bytes());
        }
        encoder.finish()
    }

    pub fn recompute_hash(&self) -> String {
        sha256_hex(&self.content_canonical_bytes())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub node_id: Option<String>,
    pub level: String,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckResult {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorValidation {
    pub validator_id: String,
    pub validator_name: String,
    pub active: bool,
    pub accepted: bool,
    pub checks: Vec<CheckResult>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsensusAttempt {
    pub round: u64,
    pub proposer_id: String,
    pub proposer_name: String,
    pub block_hash: Option<String>,
    pub validations: Vec<ValidatorValidation>,
    pub votes: Vec<Vote>,
    pub quorum_required: usize,
    pub quorum_reached: bool,
    pub confirmed: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorSummary {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub active: bool,
    pub chain_height: u64,
    pub last_hash: String,
    pub mempool_size: usize,
    pub state_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockSummary {
    pub height: u64,
    pub hash: String,
    pub previous_hash: String,
    pub transaction_count: usize,
    pub proposer: String,
    pub round: u64,
    pub vote_count: usize,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub protocol: String,
    pub simulation_notice: String,
    pub current_round: u64,
    pub current_proposer_id: String,
    pub current_proposer_name: String,
    pub quorum_required: usize,
    pub validators: Vec<ValidatorSummary>,
    pub accounts: Vec<Account>,
    pub mempool: Vec<Transaction>,
    pub blocks: Vec<BlockSummary>,
    pub last_consensus: Option<ConsensusAttempt>,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainIssue {
    pub height: Option<u64>,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainIntegrityReport {
    pub node_id: String,
    pub node_name: String,
    pub valid: bool,
    pub checked_blocks: usize,
    pub stored_state_hash: String,
    pub replayed_state_hash: String,
    pub issues: Vec<ChainIssue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrityOverview {
    pub valid: bool,
    pub replicas_consistent: bool,
    pub reports: Vec<ChainIntegrityReport>,
    pub consistency_notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScenarioReport {
    pub name: String,
    pub title: String,
    pub passed: bool,
    pub summary: String,
    pub observations: Vec<String>,
    pub snapshot: NetworkSnapshot,
    pub integrity: IntegrityOverview,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateTransactionRequest {
    pub sender: String,
    pub recipient: String,
    pub amount: u64,
    pub nonce: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ValidatorStatusRequest {
    pub active: bool,
}

pub type PublicKeyRegistry = BTreeMap<String, String>;
