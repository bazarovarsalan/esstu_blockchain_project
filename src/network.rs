use std::{
    collections::{BTreeMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::SigningKey;
use thiserror::Error;

use crate::{
    crypto::{decode_fixed, merkle_root_from_ids, public_key_hex, sign_hex},
    ledger::{
        LedgerState, QUORUM, ValidationError, confirmed_transaction_ids, genesis_block,
        project_mempool, state_hash, validate_and_apply_transaction, validate_candidate,
        validate_transaction, validate_votes, validation_trace, verify_chain,
    },
    model::{
        Account, Block, BlockHeader, BlockSummary, ConsensusAttempt, Event, IntegrityOverview,
        NetworkSnapshot, PublicKeyRegistry, Transaction, ValidatorSummary, ValidatorValidation,
        Vote,
    },
};

pub const VALIDATOR_COUNT: usize = 4;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("учётная запись '{0}' не найдена")]
    UnknownAccount(String),
    #[error("для счёта '{0}' нет демонстрационного закрытого ключа")]
    WalletUnavailable(String),
    #[error("валидатор '{0}' не найден")]
    UnknownValidator(String),
    #[error("нет активного узла, относительно которого можно проверить транзакцию")]
    NoActiveNode,
    #[error("блок высоты {0} не найден")]
    BlockNotFound(u64),
    #[error("криптографическое поле имеет некорректный формат")]
    CryptoFormat,
    #[error("демонстрационная операция неприменима: {0}")]
    Demo(String),
}

#[derive(Clone, Debug)]
pub struct ValidatorNode {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub active: bool,
    pub chain: Vec<Block>,
    pub state: LedgerState,
    pub mempool: Vec<Transaction>,
    pub event_log: Vec<Event>,
}

#[derive(Clone)]
pub struct Network {
    pub nodes: Vec<ValidatorNode>,
    validator_keys: BTreeMap<String, SigningKey>,
    user_keys: BTreeMap<String, SigningKey>,
    initial_state: LedgerState,
    registry: PublicKeyRegistry,
    validator_order: Vec<String>,
    pub current_round: u64,
    pub last_consensus: Option<ConsensusAttempt>,
    events: Vec<Event>,
    next_event_sequence: u64,
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

impl Network {
    pub fn new() -> Self {
        let mut validator_keys = BTreeMap::new();
        let mut registry = BTreeMap::new();
        let mut validator_order = Vec::with_capacity(VALIDATOR_COUNT);

        for index in 0..VALIDATOR_COUNT {
            let id = format!("validator-{}", index + 1);
            let key = SigningKey::from_bytes(&[(index + 1) as u8; 32]);
            registry.insert(id.clone(), public_key_hex(&key));
            validator_order.push(id.clone());
            validator_keys.insert(id, key);
        }

        let user_specs = [
            ("Баир", 11_u8, 1_000_000_u64),
            ("Бато", 12_u8, 250_u64),
            ("Аюна", 13_u8, 100_u64),
        ];
        let mut user_keys = BTreeMap::new();
        let mut initial_state = LedgerState::new();
        for (label, seed, balance) in user_specs {
            let key = SigningKey::from_bytes(&[seed; 32]);
            let address = public_key_hex(&key);
            initial_state.insert(
                address.clone(),
                Account {
                    address: address.clone(),
                    label: label.to_string(),
                    balance,
                    nonce: 0,
                },
            );
            user_keys.insert(address, key);
        }

        let genesis = genesis_block(&initial_state);
        let nodes = validator_order
            .iter()
            .enumerate()
            .map(|(index, id)| ValidatorNode {
                id: id.clone(),
                name: format!("Validator {}", index + 1),
                public_key: registry[id].clone(),
                active: true,
                chain: vec![genesis.clone()],
                state: initial_state.clone(),
                mempool: Vec::new(),
                event_log: Vec::new(),
            })
            .collect();

        let mut network = Self {
            nodes,
            validator_keys,
            user_keys,
            initial_state,
            registry,
            validator_order,
            current_round: 0,
            last_consensus: None,
            events: Vec::new(),
            next_event_sequence: 1,
        };
        network.emit_all(
            "info",
            "network_initialized",
            "Созданы четыре логических валидатора и общий детерминированный генезис-блок",
        );
        network
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn validator_order(&self) -> &[String] {
        &self.validator_order
    }

    pub fn registry(&self) -> &PublicKeyRegistry {
        &self.registry
    }

    pub fn initial_state(&self) -> &LedgerState {
        &self.initial_state
    }

    pub fn proposer_id(&self, round: u64) -> &str {
        &self.validator_order[round as usize % self.validator_order.len()]
    }

    pub fn current_proposer_id(&self) -> &str {
        self.proposer_id(self.current_round)
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn now_seconds() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn emit_all(&mut self, level: &str, kind: &str, message: impl Into<String>) {
        let event = Event {
            sequence: self.next_event_sequence,
            timestamp_ms: Self::now_millis(),
            node_id: None,
            level: level.to_string(),
            kind: kind.to_string(),
            message: message.into(),
        };
        self.next_event_sequence += 1;
        self.events.push(event.clone());
        for node in &mut self.nodes {
            node.event_log.push(event.clone());
        }
    }

    fn emit_node(&mut self, node_id: &str, level: &str, kind: &str, message: impl Into<String>) {
        let event = Event {
            sequence: self.next_event_sequence,
            timestamp_ms: Self::now_millis(),
            node_id: Some(node_id.to_string()),
            level: level.to_string(),
            kind: kind.to_string(),
            message: message.into(),
        };
        self.next_event_sequence += 1;
        self.events.push(event.clone());
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == node_id) {
            node.event_log.push(event);
        }
    }

    fn canonical_node_index(&self) -> Option<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.active)
            .max_by_key(|(_, node)| node.chain.len())
            .map(|(index, _)| index)
            .or_else(|| {
                self.nodes
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, node)| node.chain.len())
                    .map(|(index, _)| index)
            })
    }

    pub fn resolve_account(&self, reference: &str) -> Result<String, NetworkError> {
        let canonical = self
            .canonical_node_index()
            .ok_or(NetworkError::NoActiveNode)?;
        let normalized_reference = reference.to_lowercase();
        self.nodes[canonical]
            .state
            .values()
            .find(|account| {
                account.address == reference || account.label.to_lowercase() == normalized_reference
            })
            .map(|account| account.address.clone())
            .ok_or_else(|| NetworkError::UnknownAccount(reference.to_string()))
    }

    pub fn make_signed_transaction(
        &self,
        sender_reference: &str,
        recipient_reference: &str,
        amount: u64,
        nonce: Option<u64>,
    ) -> Result<Transaction, NetworkError> {
        let sender = self.resolve_account(sender_reference)?;
        let recipient = self.resolve_account(recipient_reference)?;
        let signing_key = self
            .user_keys
            .get(&sender)
            .ok_or_else(|| NetworkError::WalletUnavailable(sender_reference.to_string()))?;
        let canonical = self
            .canonical_node_index()
            .ok_or(NetworkError::NoActiveNode)?;
        let projected = project_mempool(
            &self.nodes[canonical].state,
            &self.nodes[canonical].chain,
            &self.nodes[canonical].mempool,
        )?;
        let nonce = nonce.unwrap_or(
            projected
                .get(&sender)
                .ok_or_else(|| NetworkError::UnknownAccount(sender_reference.to_string()))?
                .nonce,
        );
        Ok(Transaction::new_signed(
            signing_key,
            recipient,
            amount,
            nonce,
        ))
    }

    pub fn create_and_submit_transaction(
        &mut self,
        sender: &str,
        recipient: &str,
        amount: u64,
        nonce: Option<u64>,
    ) -> Result<Transaction, NetworkError> {
        let transaction = self.make_signed_transaction(sender, recipient, amount, nonce)?;
        self.submit_transaction(transaction.clone())?;
        Ok(transaction)
    }

    /// Создаёт последовательный пакет учебных транзакций и проверяет его за
    /// один проход. Метод нужен для воспроизводимого эксперимента с блоками
    /// разного размера; каждая подпись, nonce и переход баланса проверяются.
    pub fn create_and_submit_batch(
        &mut self,
        sender_reference: &str,
        recipient_reference: &str,
        amount: u64,
        count: usize,
    ) -> Result<Vec<Transaction>, NetworkError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let sender = self.resolve_account(sender_reference)?;
        let recipient = self.resolve_account(recipient_reference)?;
        let signing_key = self
            .user_keys
            .get(&sender)
            .ok_or_else(|| NetworkError::WalletUnavailable(sender_reference.to_string()))?;
        let canonical = self
            .canonical_node_index()
            .ok_or(NetworkError::NoActiveNode)?;
        let projected = project_mempool(
            &self.nodes[canonical].state,
            &self.nodes[canonical].chain,
            &self.nodes[canonical].mempool,
        )?;
        let first_nonce = projected
            .get(&sender)
            .ok_or_else(|| NetworkError::UnknownAccount(sender_reference.to_string()))?
            .nonce;
        let mut transactions = Vec::with_capacity(count);
        for offset in 0..count {
            let nonce = first_nonce
                .checked_add(
                    u64::try_from(offset).map_err(|_| ValidationError::ArithmeticOverflow)?,
                )
                .ok_or(ValidationError::ArithmeticOverflow)?;
            transactions.push(Transaction::new_signed(
                signing_key,
                recipient.clone(),
                amount,
                nonce,
            ));
        }
        self.submit_transaction_batch(&transactions)?;
        Ok(transactions)
    }

    pub fn submit_transaction_batch(
        &mut self,
        transactions: &[Transaction],
    ) -> Result<(), NetworkError> {
        if transactions.is_empty() {
            return Ok(());
        }
        let active_indices = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.active)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if active_indices.is_empty() {
            return Err(NetworkError::NoActiveNode);
        }

        for index in &active_indices {
            let node = &self.nodes[*index];
            let mut projected = project_mempool(&node.state, &node.chain, &node.mempool)?;
            let mut seen = confirmed_transaction_ids(&node.chain);
            seen.extend(node.mempool.iter().map(|tx| tx.id.clone()));
            for transaction in transactions {
                validate_and_apply_transaction(transaction, &mut projected, &mut seen)?;
            }
        }
        for index in active_indices {
            self.nodes[index].mempool.extend_from_slice(transactions);
        }
        self.emit_all(
            "info",
            "transaction_batch_accepted",
            format!(
                "Пакет из {} транзакций проверен и добавлен в пулы активных узлов",
                transactions.len()
            ),
        );
        Ok(())
    }

    pub fn submit_transaction(&mut self, transaction: Transaction) -> Result<(), NetworkError> {
        let active_indices = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.active)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if active_indices.is_empty() {
            return Err(NetworkError::NoActiveNode);
        }

        for index in &active_indices {
            let node = &self.nodes[*index];
            let projected = project_mempool(&node.state, &node.chain, &node.mempool)?;
            let mut seen = confirmed_transaction_ids(&node.chain);
            seen.extend(node.mempool.iter().map(|tx| tx.id.clone()));
            if let Err(error) = validate_transaction(&transaction, &projected, &seen) {
                self.emit_all(
                    "error",
                    "transaction_rejected",
                    format!("Транзакция отклонена до пула: {error}"),
                );
                return Err(error.into());
            }
        }

        for index in active_indices {
            self.nodes[index].mempool.push(transaction.clone());
        }
        self.emit_all(
            "info",
            "transaction_accepted",
            format!(
                "Транзакция {} проверена и добавлена в пулы активных узлов",
                transaction.id
            ),
        );
        Ok(())
    }

    /// Формирует и подписывает кандидат без голосования и изменения состояния.
    /// Публичный метод используется интеграционными тестами и экспериментом.
    pub fn build_candidate(&self) -> Result<Block, NetworkError> {
        let proposer_id = self.current_proposer_id().to_string();
        let proposer = self
            .nodes
            .iter()
            .find(|node| node.id == proposer_id)
            .ok_or_else(|| NetworkError::UnknownValidator(proposer_id.clone()))?;
        if !proposer.active {
            return Err(ValidationError::InactiveProposer.into());
        }
        if proposer.mempool.is_empty() {
            return Err(ValidationError::EmptyMempool.into());
        }

        let projected = project_mempool(&proposer.state, &proposer.chain, &proposer.mempool)?;
        let ids = proposer
            .mempool
            .iter()
            .map(|transaction| transaction.id.clone())
            .collect::<Vec<_>>();
        let root = merkle_root_from_ids(&ids).map_err(|_| NetworkError::CryptoFormat)?;
        let mut block = Block {
            hash: String::new(),
            header: BlockHeader {
                height: proposer.chain.len() as u64,
                previous_hash: proposer.chain.last().expect("genesis exists").hash.clone(),
                transactions_root: root,
                state_hash: state_hash(&projected),
                timestamp: Self::now_seconds(),
                round: self.current_round,
                proposer: proposer_id.clone(),
            },
            transactions: proposer.mempool.clone(),
            proposer_signature: String::new(),
            votes: Vec::new(),
        };
        block.hash = block.recompute_hash();
        let block_hash = decode_fixed::<32>(&block.hash).map_err(|_| NetworkError::CryptoFormat)?;
        block.proposer_signature = sign_hex(
            self.validator_keys
                .get(&proposer_id)
                .expect("registered proposer key"),
            &block_hash,
        );
        Ok(block)
    }

    fn failed_attempt(&self, error: &ValidationError) -> ConsensusAttempt {
        let proposer_id = self.current_proposer_id().to_string();
        let proposer_name = self
            .nodes
            .iter()
            .find(|node| node.id == proposer_id)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| proposer_id.clone());
        ConsensusAttempt {
            round: self.current_round,
            proposer_id,
            proposer_name,
            block_hash: None,
            validations: Vec::new(),
            votes: Vec::new(),
            quorum_required: QUORUM,
            quorum_reached: false,
            confirmed: false,
            reason: error.to_string(),
        }
    }

    pub(crate) fn attempt_candidate(&mut self, mut block: Block) -> ConsensusAttempt {
        let round = self.current_round;
        let expected_proposer = self.current_proposer_id().to_string();
        let proposer_name = self
            .nodes
            .iter()
            .find(|node| node.id == expected_proposer)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| expected_proposer.clone());

        self.emit_all(
            "info",
            "block_proposed",
            format!(
                "{} сформировал кандидат блока {} в раунде {}",
                proposer_name, block.hash, round
            ),
        );

        let mut validations = Vec::with_capacity(self.nodes.len());
        let mut accepted_ids = Vec::new();
        for node in &self.nodes {
            if !node.active {
                validations.push(ValidatorValidation {
                    validator_id: node.id.clone(),
                    validator_name: node.name.clone(),
                    active: false,
                    accepted: false,
                    checks: Vec::new(),
                    error: Some("валидатор отключён и не выполнял проверку".to_string()),
                });
                continue;
            }
            let result = validate_candidate(
                &block,
                &node.chain,
                &node.state,
                &self.registry,
                &expected_proposer,
            );
            let accepted = result.is_ok();
            let error = result.as_ref().err().map(ToString::to_string);
            if accepted {
                accepted_ids.push(node.id.clone());
            }
            validations.push(ValidatorValidation {
                validator_id: node.id.clone(),
                validator_name: node.name.clone(),
                active: true,
                accepted,
                checks: validation_trace(&result),
                error,
            });
        }

        let votes = accepted_ids
            .iter()
            .map(|validator_id| {
                Vote::signed(
                    validator_id.clone(),
                    block.hash.clone(),
                    round,
                    self.validator_keys
                        .get(validator_id)
                        .expect("registered validator key"),
                )
            })
            .collect::<Vec<_>>();
        block.votes = votes.clone();
        let vote_check = validate_votes(&block, &self.registry, true);
        let quorum_reached = vote_check.is_ok();
        let confirmed = quorum_reached;

        let reason = if confirmed {
            format!(
                "Кворум достигнут: {} корректных голосов из {}",
                votes.len(),
                VALIDATOR_COUNT
            )
        } else if let Err(error) = &vote_check {
            error.to_string()
        } else {
            "Блок отклонён".to_string()
        };

        if confirmed {
            let included = block
                .transactions
                .iter()
                .map(|transaction| transaction.id.clone())
                .collect::<HashSet<_>>();
            for node in &mut self.nodes {
                if !node.active {
                    continue;
                }
                let projected = validate_candidate(
                    &block,
                    &node.chain,
                    &node.state,
                    &self.registry,
                    &expected_proposer,
                )
                .expect("node already accepted identical candidate");
                node.state = projected;
                node.chain.push(block.clone());
                node.mempool
                    .retain(|transaction| !included.contains(&transaction.id));
            }
            self.emit_all(
                "info",
                "block_committed",
                format!(
                    "Блок {} подтверждён {} голосами и добавлен в цепочки активных узлов",
                    block.hash,
                    votes.len()
                ),
            );
        } else {
            self.emit_all(
                "warning",
                "block_not_committed",
                format!("Блок не подтверждён: {reason}"),
            );
        }

        let attempt = ConsensusAttempt {
            round,
            proposer_id: expected_proposer,
            proposer_name,
            block_hash: Some(block.hash),
            validations,
            votes,
            quorum_required: QUORUM,
            quorum_reached,
            confirmed,
            reason,
        };
        self.current_round += 1;
        self.last_consensus = Some(attempt.clone());
        attempt
    }

    pub fn produce_block(&mut self) -> Result<ConsensusAttempt, NetworkError> {
        match self.build_candidate() {
            Ok(block) => Ok(self.attempt_candidate(block)),
            Err(NetworkError::Validation(error @ ValidationError::InactiveProposer))
            | Err(NetworkError::Validation(error @ ValidationError::EmptyMempool)) => {
                let attempt = self.failed_attempt(&error);
                self.emit_all(
                    "warning",
                    "round_failed",
                    format!("Раунд {} завершён без блока: {error}", self.current_round),
                );
                self.current_round += 1;
                self.last_consensus = Some(attempt.clone());
                Ok(attempt)
            }
            Err(error) => Err(error),
        }
    }

    pub fn set_validator_active(
        &mut self,
        validator_reference: &str,
        active: bool,
    ) -> Result<(), NetworkError> {
        let normalized_reference = validator_reference.to_lowercase();
        let target = self
            .nodes
            .iter()
            .position(|node| {
                node.id == validator_reference || node.name.to_lowercase() == normalized_reference
            })
            .ok_or_else(|| NetworkError::UnknownValidator(validator_reference.to_string()))?;

        if active && !self.nodes[target].active {
            let source = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(index, node)| *index != target && node.active)
                .max_by_key(|(_, node)| node.chain.len())
                .map(|(index, _)| index);
            if let Some(source) = source {
                self.nodes[target].chain = self.nodes[source].chain.clone();
                self.nodes[target].state = self.nodes[source].state.clone();
                self.nodes[target].mempool = self.nodes[source].mempool.clone();
            }
            self.nodes[target].active = true;
            let id = self.nodes[target].id.clone();
            self.emit_node(
                &id,
                "info",
                "validator_enabled",
                "Валидатор включён и синхронизирован с наиболее высокой активной репликой",
            );
        } else if !active && self.nodes[target].active {
            self.nodes[target].active = false;
            let id = self.nodes[target].id.clone();
            self.emit_node(
                &id,
                "warning",
                "validator_disabled",
                "Валидатор отключён и не участвует в проверке и голосовании",
            );
        }
        Ok(())
    }

    pub fn block_at(&self, height: u64) -> Result<Block, NetworkError> {
        let canonical = self
            .canonical_node_index()
            .ok_or(NetworkError::NoActiveNode)?;
        self.nodes[canonical]
            .chain
            .iter()
            .find(|block| block.header.height == height)
            .cloned()
            .ok_or(NetworkError::BlockNotFound(height))
    }

    pub fn integrity(&self) -> IntegrityOverview {
        let reports = self
            .nodes
            .iter()
            .map(|node| {
                verify_chain(
                    &node.id,
                    &node.name,
                    &node.chain,
                    &node.state,
                    &self.initial_state,
                    &self.registry,
                    &self.validator_order,
                )
            })
            .collect::<Vec<_>>();

        let active = self
            .nodes
            .iter()
            .filter(|node| node.active)
            .collect::<Vec<_>>();
        let mut notes = Vec::new();
        let mut replicas_consistent = true;
        if let Some(reference) = active.first() {
            let reference_chain = reference
                .chain
                .iter()
                .map(Block::recompute_hash)
                .collect::<Vec<_>>();
            let reference_state = state_hash(&reference.state);
            for node in active.iter().skip(1) {
                let node_chain = node
                    .chain
                    .iter()
                    .map(Block::recompute_hash)
                    .collect::<Vec<_>>();
                if node_chain != reference_chain {
                    replicas_consistent = false;
                    notes.push(format!(
                        "Цепочка {} отличается от цепочки {}",
                        node.name, reference.name
                    ));
                }
                if state_hash(&node.state) != reference_state {
                    replicas_consistent = false;
                    notes.push(format!(
                        "Состояние {} отличается от состояния {}",
                        node.name, reference.name
                    ));
                }
            }
        }
        if replicas_consistent {
            notes.push("Активные реплики имеют одинаковые цепочки и состояния".to_string());
        }
        let valid = replicas_consistent && reports.iter().all(|report| report.valid);
        IntegrityOverview {
            valid,
            replicas_consistent,
            reports,
            consistency_notes: notes,
        }
    }

    pub fn snapshot(&self) -> NetworkSnapshot {
        let canonical = self.canonical_node_index().unwrap_or(0);
        let canonical_node = &self.nodes[canonical];
        let validators = self
            .nodes
            .iter()
            .map(|node| ValidatorSummary {
                id: node.id.clone(),
                name: node.name.clone(),
                public_key: node.public_key.clone(),
                active: node.active,
                chain_height: node
                    .chain
                    .last()
                    .map(|block| block.header.height)
                    .unwrap_or(0),
                last_hash: node
                    .chain
                    .last()
                    .map(|block| block.hash.clone())
                    .unwrap_or_default(),
                mempool_size: node.mempool.len(),
                state_hash: state_hash(&node.state),
            })
            .collect();
        let blocks = canonical_node
            .chain
            .iter()
            .map(|block| BlockSummary {
                height: block.header.height,
                hash: block.hash.clone(),
                previous_hash: block.header.previous_hash.clone(),
                transaction_count: block.transactions.len(),
                proposer: block.header.proposer.clone(),
                round: block.header.round,
                vote_count: block.votes.len(),
                timestamp: block.header.timestamp,
            })
            .collect();
        let proposer_id = self.current_proposer_id().to_string();
        let proposer_name = self
            .nodes
            .iter()
            .find(|node| node.id == proposer_id)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| proposer_id.clone());
        NetworkSnapshot {
            protocol: "RoundRobinQuorum".to_string(),
            simulation_notice: "Учебная имитация четырёх логических узлов в одном Rust-процессе; не является полноценной BFT- или P2P-сетью".to_string(),
            current_round: self.current_round,
            current_proposer_id: proposer_id,
            current_proposer_name: proposer_name,
            quorum_required: QUORUM,
            validators,
            accounts: canonical_node.state.values().cloned().collect(),
            mempool: canonical_node.mempool.clone(),
            blocks,
            last_consensus: self.last_consensus.clone(),
            events: self
                .events
                .iter()
                .rev()
                .take(200)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        }
    }

    pub(crate) fn demo_tamper_transaction(
        &mut self,
        validator_id: &str,
        height: u64,
        transaction_index: usize,
    ) -> Result<(), NetworkError> {
        let node = self
            .nodes
            .iter_mut()
            .find(|node| node.id == validator_id)
            .ok_or_else(|| NetworkError::UnknownValidator(validator_id.to_string()))?;
        let block = node
            .chain
            .iter_mut()
            .find(|block| block.header.height == height)
            .ok_or(NetworkError::BlockNotFound(height))?;
        let transaction = block
            .transactions
            .get_mut(transaction_index)
            .ok_or_else(|| {
                NetworkError::Demo("в выбранном блоке нет указанной транзакции".to_string())
            })?;
        transaction.amount = transaction
            .amount
            .checked_add(1)
            .ok_or_else(|| NetworkError::Demo("сумма не может быть увеличена".to_string()))?;
        self.emit_node(
            validator_id,
            "error",
            "demo_tamper",
            format!(
                "Демонстрационная функция изменила сумму транзакции в локальной копии блока {height}; служебные хеши намеренно не пересчитаны"
            ),
        );
        Ok(())
    }

    pub(crate) fn demo_invalid_block(
        &mut self,
        field: &str,
    ) -> Result<ConsensusAttempt, NetworkError> {
        let mut block = self.build_candidate()?;
        match field {
            "previous_hash" => block.header.previous_hash = "ff".repeat(32),
            "state_hash" => block.header.state_hash = "ff".repeat(32),
            "transaction" => {
                let transaction = block.transactions.first_mut().ok_or_else(|| {
                    NetworkError::Demo("для подмены требуется транзакция".to_string())
                })?;
                transaction.amount = transaction.amount.saturating_add(1);
            }
            other => {
                return Err(NetworkError::Demo(format!(
                    "неизвестное поле подмены: {other}"
                )));
            }
        }
        Ok(self.attempt_candidate(block))
    }

    pub fn active_validator_count(&self) -> usize {
        self.nodes.iter().filter(|node| node.active).count()
    }

    pub fn validate_candidate_on_active_nodes(
        &self,
        block: &Block,
    ) -> Vec<Result<LedgerState, ValidationError>> {
        let expected = self.current_proposer_id();
        self.nodes
            .iter()
            .filter(|node| node.active)
            .map(|node| {
                validate_candidate(block, &node.chain, &node.state, &self.registry, expected)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposer_is_selected_round_robin() {
        let network = Network::new();
        assert_eq!(network.proposer_id(0), "validator-1");
        assert_eq!(network.proposer_id(1), "validator-2");
        assert_eq!(network.proposer_id(4), "validator-1");
    }

    #[test]
    fn four_votes_confirm_block_and_replicas_match() {
        let mut network = Network::new();
        network
            .create_and_submit_transaction("Баир", "Бато", 10, None)
            .unwrap();
        let result = network.produce_block().unwrap();
        assert!(result.confirmed);
        assert_eq!(result.votes.len(), 4);
        assert!(network.integrity().valid);
    }

    #[test]
    fn three_votes_are_enough() {
        let mut network = Network::new();
        network.set_validator_active("validator-4", false).unwrap();
        network
            .create_and_submit_transaction("Баир", "Бато", 10, None)
            .unwrap();
        let result = network.produce_block().unwrap();
        assert!(result.confirmed);
        assert_eq!(result.votes.len(), 3);
    }

    #[test]
    fn two_votes_do_not_confirm_block() {
        let mut network = Network::new();
        network.set_validator_active("validator-3", false).unwrap();
        network.set_validator_active("validator-4", false).unwrap();
        network
            .create_and_submit_transaction("Баир", "Бато", 10, None)
            .unwrap();
        let result = network.produce_block().unwrap();
        assert!(!result.confirmed);
        assert_eq!(result.votes.len(), 2);
        assert_eq!(network.nodes[0].chain.len(), 1);
    }

    #[test]
    fn invalid_previous_hash_is_rejected_by_every_active_validator() {
        let mut network = Network::new();
        network
            .create_and_submit_transaction("Баир", "Бато", 10, None)
            .unwrap();
        let result = network.demo_invalid_block("previous_hash").unwrap();
        assert!(!result.confirmed);
        assert!(
            result
                .validations
                .iter()
                .filter(|validation| validation.active)
                .all(|validation| !validation.accepted)
        );
    }
}
