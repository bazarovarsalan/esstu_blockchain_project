use std::collections::{BTreeMap, HashSet};

use thiserror::Error;

use crate::{
    canonical::CanonicalEncoder,
    crypto::{decode_fixed, merkle_root_from_ids, sha256_hex, verify_hex},
    model::{
        Account, Block, BlockHeader, ChainIntegrityReport, ChainIssue, CheckResult,
        PublicKeyRegistry, Transaction,
    },
};

pub type LedgerState = BTreeMap<String, Account>;

pub const QUORUM: usize = 3;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("сумма транзакции должна быть больше нуля")]
    ZeroAmount,
    #[error("идентификатор транзакции не соответствует её каноническим данным")]
    TransactionId,
    #[error("подпись транзакции Ed25519 некорректна")]
    TransactionSignature,
    #[error("счёт отправителя не зарегистрирован")]
    UnknownSender,
    #[error("счёт получателя не зарегистрирован")]
    UnknownRecipient,
    #[error("транзакция {0} уже присутствует в истории или проверяемом наборе")]
    DuplicateTransaction(String),
    #[error("неверный nonce: ожидался {expected}, получен {actual}")]
    Nonce { expected: u64, actual: u64 },
    #[error("недостаточный баланс: доступно {available}, требуется {required}")]
    InsufficientBalance { available: u64, required: u64 },
    #[error("арифметическое переполнение баланса или nonce")]
    ArithmeticOverflow,
    #[error("неверная высота блока: ожидалась {expected}, получена {actual}")]
    Height { expected: u64, actual: u64 },
    #[error("previous_hash не соответствует вершине локальной цепочки")]
    PreviousHash,
    #[error("автор блока не зарегистрирован в консорциуме")]
    UnknownProposer,
    #[error("автор блока не соответствует текущему раунду: ожидался {expected}")]
    WrongProposer { expected: String },
    #[error("сохранённый хеш блока не соответствует его каноническому содержимому")]
    BlockHash,
    #[error("подпись автора блока Ed25519 некорректна")]
    ProposerSignature,
    #[error("корень транзакций не соответствует содержимому блока")]
    TransactionsRoot,
    #[error("state_hash не соответствует результату исполнения транзакций")]
    StateHash,
    #[error("голос принадлежит незарегистрированному валидатору")]
    UnknownVoter,
    #[error("обнаружен повторный голос валидатора {0}")]
    DuplicateVote(String),
    #[error("голос относится к другому блоку или раунду")]
    VoteTarget,
    #[error("учтён отрицательный голос")]
    NegativeVote,
    #[error("подпись голоса Ed25519 некорректна")]
    VoteSignature,
    #[error("кворум не достигнут: получено {actual}, требуется {required}")]
    Quorum { actual: usize, required: usize },
    #[error("пул транзакций пуст")]
    EmptyMempool,
    #[error("автор текущего раунда отключён")]
    InactiveProposer,
}

impl ValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ZeroAmount => "zero_amount",
            Self::TransactionId => "transaction_id",
            Self::TransactionSignature => "transaction_signature",
            Self::UnknownSender => "unknown_sender",
            Self::UnknownRecipient => "unknown_recipient",
            Self::DuplicateTransaction(_) => "duplicate_transaction",
            Self::Nonce { .. } => "nonce",
            Self::InsufficientBalance { .. } => "insufficient_balance",
            Self::ArithmeticOverflow => "arithmetic_overflow",
            Self::Height { .. } => "height",
            Self::PreviousHash => "previous_hash",
            Self::UnknownProposer => "proposer_authority",
            Self::WrongProposer { .. } => "proposer_round",
            Self::BlockHash => "block_hash",
            Self::ProposerSignature => "proposer_signature",
            Self::TransactionsRoot => "transactions_root",
            Self::StateHash => "state_hash",
            Self::UnknownVoter => "vote_authority",
            Self::DuplicateVote(_) => "vote_duplicate",
            Self::VoteTarget => "vote_target",
            Self::NegativeVote => "vote_decision",
            Self::VoteSignature => "vote_signature",
            Self::Quorum { .. } => "quorum",
            Self::EmptyMempool => "mempool",
            Self::InactiveProposer => "proposer_active",
        }
    }

    pub fn candidate_stage(&self) -> &'static str {
        match self {
            Self::Height { .. } => "height",
            Self::PreviousHash => "previous_hash",
            Self::UnknownProposer => "proposer_authority",
            Self::WrongProposer { .. } => "proposer_round",
            Self::BlockHash => "block_hash",
            Self::ProposerSignature => "proposer_signature",
            Self::DuplicateTransaction(_) => "duplicate_transactions",
            Self::TransactionsRoot => "transactions_root",
            Self::StateHash => "state_hash",
            Self::ZeroAmount
            | Self::TransactionId
            | Self::TransactionSignature
            | Self::UnknownSender
            | Self::UnknownRecipient
            | Self::Nonce { .. }
            | Self::InsufficientBalance { .. }
            | Self::ArithmeticOverflow => "transaction_rules",
            _ => "transaction_rules",
        }
    }
}

pub fn state_hash(state: &LedgerState) -> String {
    let mut encoder = CanonicalEncoder::new("RRQ/STATE");
    encoder.put_u32(u32::try_from(state.len()).expect("too many accounts"));
    for (address, account) in state {
        encoder.put_str(address);
        encoder.put_u64(account.balance);
        encoder.put_u64(account.nonce);
    }
    sha256_hex(&encoder.finish())
}

pub fn confirmed_transaction_ids(chain: &[Block]) -> HashSet<String> {
    chain
        .iter()
        .flat_map(|block| {
            block
                .transactions
                .iter()
                .map(|transaction| transaction.id.clone())
        })
        .collect()
}

pub fn validate_transaction(
    transaction: &Transaction,
    state: &LedgerState,
    seen: &HashSet<String>,
) -> Result<(), ValidationError> {
    if transaction.amount == 0 {
        return Err(ValidationError::ZeroAmount);
    }
    if transaction.recompute_id() != transaction.id {
        return Err(ValidationError::TransactionId);
    }
    verify_hex(
        &transaction.sender,
        &transaction.unsigned_canonical_bytes(),
        &transaction.signature,
    )
    .map_err(|_| ValidationError::TransactionSignature)?;
    let sender = state
        .get(&transaction.sender)
        .ok_or(ValidationError::UnknownSender)?;
    let recipient = state
        .get(&transaction.recipient)
        .ok_or(ValidationError::UnknownRecipient)?;
    if seen.contains(&transaction.id) {
        return Err(ValidationError::DuplicateTransaction(
            transaction.id.clone(),
        ));
    }
    if sender.nonce != transaction.nonce {
        return Err(ValidationError::Nonce {
            expected: sender.nonce,
            actual: transaction.nonce,
        });
    }
    if sender.balance < transaction.amount {
        return Err(ValidationError::InsufficientBalance {
            available: sender.balance,
            required: transaction.amount,
        });
    }
    sender
        .nonce
        .checked_add(1)
        .ok_or(ValidationError::ArithmeticOverflow)?;
    if transaction.sender != transaction.recipient {
        recipient
            .balance
            .checked_add(transaction.amount)
            .ok_or(ValidationError::ArithmeticOverflow)?;
    }
    Ok(())
}

pub fn apply_transaction(
    transaction: &Transaction,
    state: &mut LedgerState,
) -> Result<(), ValidationError> {
    if transaction.sender == transaction.recipient {
        let account = state
            .get_mut(&transaction.sender)
            .ok_or(ValidationError::UnknownSender)?;
        account.nonce = account
            .nonce
            .checked_add(1)
            .ok_or(ValidationError::ArithmeticOverflow)?;
        return Ok(());
    }

    {
        let sender = state
            .get_mut(&transaction.sender)
            .ok_or(ValidationError::UnknownSender)?;
        sender.balance -= transaction.amount;
        sender.nonce = sender
            .nonce
            .checked_add(1)
            .ok_or(ValidationError::ArithmeticOverflow)?;
    }
    let recipient = state
        .get_mut(&transaction.recipient)
        .ok_or(ValidationError::UnknownRecipient)?;
    recipient.balance = recipient
        .balance
        .checked_add(transaction.amount)
        .ok_or(ValidationError::ArithmeticOverflow)?;
    Ok(())
}

pub fn validate_and_apply_transaction(
    transaction: &Transaction,
    state: &mut LedgerState,
    seen: &mut HashSet<String>,
) -> Result<(), ValidationError> {
    validate_transaction(transaction, state, seen)?;
    apply_transaction(transaction, state)?;
    seen.insert(transaction.id.clone());
    Ok(())
}

pub fn project_mempool(
    state: &LedgerState,
    chain: &[Block],
    mempool: &[Transaction],
) -> Result<LedgerState, ValidationError> {
    let mut projected = state.clone();
    let mut seen = confirmed_transaction_ids(chain);
    for transaction in mempool {
        validate_and_apply_transaction(transaction, &mut projected, &mut seen)?;
    }
    Ok(projected)
}

pub fn genesis_block(initial_state: &LedgerState) -> Block {
    let mut block = Block {
        hash: String::new(),
        header: BlockHeader {
            height: 0,
            previous_hash: "00".repeat(32),
            transactions_root: merkle_root_from_ids(&[]).expect("empty Merkle root"),
            state_hash: state_hash(initial_state),
            timestamp: 0,
            round: 0,
            proposer: "GENESIS".to_string(),
        },
        transactions: Vec::new(),
        proposer_signature: String::new(),
        votes: Vec::new(),
    };
    block.hash = block.recompute_hash();
    block
}

pub fn validate_candidate(
    block: &Block,
    chain: &[Block],
    state: &LedgerState,
    registry: &PublicKeyRegistry,
    expected_proposer: &str,
) -> Result<LedgerState, ValidationError> {
    let expected_height = chain.len() as u64;
    if block.header.height != expected_height {
        return Err(ValidationError::Height {
            expected: expected_height,
            actual: block.header.height,
        });
    }
    let last_hash = chain
        .last()
        .map(|last| last.hash.as_str())
        .unwrap_or_default();
    if block.header.previous_hash != last_hash {
        return Err(ValidationError::PreviousHash);
    }
    let proposer_key = registry
        .get(&block.header.proposer)
        .ok_or(ValidationError::UnknownProposer)?;
    if block.header.proposer != expected_proposer {
        return Err(ValidationError::WrongProposer {
            expected: expected_proposer.to_string(),
        });
    }
    if block.recompute_hash() != block.hash {
        return Err(ValidationError::BlockHash);
    }
    let block_hash = decode_fixed::<32>(&block.hash).map_err(|_| ValidationError::BlockHash)?;
    verify_hex(proposer_key, &block_hash, &block.proposer_signature)
        .map_err(|_| ValidationError::ProposerSignature)?;

    let mut block_ids = HashSet::new();
    for transaction in &block.transactions {
        if !block_ids.insert(transaction.id.clone()) {
            return Err(ValidationError::DuplicateTransaction(
                transaction.id.clone(),
            ));
        }
    }

    let ids = block
        .transactions
        .iter()
        .map(|transaction| transaction.id.clone())
        .collect::<Vec<_>>();
    let root = merkle_root_from_ids(&ids).map_err(|_| ValidationError::TransactionsRoot)?;
    if root != block.header.transactions_root {
        return Err(ValidationError::TransactionsRoot);
    }

    let mut projected = state.clone();
    let mut seen = confirmed_transaction_ids(chain);
    for transaction in &block.transactions {
        validate_and_apply_transaction(transaction, &mut projected, &mut seen)?;
    }
    if state_hash(&projected) != block.header.state_hash {
        return Err(ValidationError::StateHash);
    }
    Ok(projected)
}

pub fn validate_votes(
    block: &Block,
    registry: &PublicKeyRegistry,
    require_quorum: bool,
) -> Result<usize, ValidationError> {
    let mut voters = HashSet::new();
    for vote in &block.votes {
        let public_key = registry
            .get(&vote.validator_id)
            .ok_or(ValidationError::UnknownVoter)?;
        if !voters.insert(vote.validator_id.clone()) {
            return Err(ValidationError::DuplicateVote(vote.validator_id.clone()));
        }
        if vote.block_hash != block.hash || vote.round != block.header.round {
            return Err(ValidationError::VoteTarget);
        }
        if !vote.approve {
            return Err(ValidationError::NegativeVote);
        }
        verify_hex(public_key, &vote.canonical_message(), &vote.signature)
            .map_err(|_| ValidationError::VoteSignature)?;
    }
    if require_quorum && voters.len() < QUORUM {
        return Err(ValidationError::Quorum {
            actual: voters.len(),
            required: QUORUM,
        });
    }
    Ok(voters.len())
}

const CANDIDATE_STAGES: [(&str, &str); 11] = [
    ("height", "Высота блока"),
    ("previous_hash", "Хеш предыдущего блока"),
    ("proposer_authority", "Полномочия автора"),
    ("proposer_round", "Автор текущего раунда"),
    ("block_hash", "Хеш содержимого блока"),
    ("proposer_signature", "Подпись автора блока"),
    ("duplicate_transactions", "Отсутствие дубликатов"),
    ("transactions_root", "Корень транзакций"),
    ("transaction_rules", "Подписи, nonce и балансы транзакций"),
    (
        "state_transition",
        "Детерминированное применение транзакций",
    ),
    ("state_hash", "Хеш итогового состояния"),
];

pub fn validation_trace(result: &Result<LedgerState, ValidationError>) -> Vec<CheckResult> {
    match result {
        Ok(_) => CANDIDATE_STAGES
            .iter()
            .map(|(name, title)| CheckResult {
                name: (*name).to_string(),
                status: "passed".to_string(),
                detail: format!("{title}: проверка пройдена"),
            })
            .collect(),
        Err(error) => {
            let failed_stage = error.candidate_stage();
            let failed_index = CANDIDATE_STAGES
                .iter()
                .position(|(name, _)| *name == failed_stage)
                .unwrap_or(0);
            CANDIDATE_STAGES
                .iter()
                .enumerate()
                .map(|(index, (name, title))| {
                    if index < failed_index {
                        CheckResult {
                            name: (*name).to_string(),
                            status: "passed".to_string(),
                            detail: format!("{title}: проверка пройдена"),
                        }
                    } else if index == failed_index {
                        CheckResult {
                            name: (*name).to_string(),
                            status: "failed".to_string(),
                            detail: error.to_string(),
                        }
                    } else {
                        CheckResult {
                            name: (*name).to_string(),
                            status: "not_reached".to_string(),
                            detail: format!("{title}: проверка не выполнялась после отказа"),
                        }
                    }
                })
                .collect()
        }
    }
}

fn push_issue(
    issues: &mut Vec<ChainIssue>,
    height: Option<u64>,
    code: impl Into<String>,
    message: impl Into<String>,
) {
    issues.push(ChainIssue {
        height,
        code: code.into(),
        message: message.into(),
    });
}

pub fn verify_chain(
    node_id: &str,
    node_name: &str,
    chain: &[Block],
    stored_state: &LedgerState,
    initial_state: &LedgerState,
    registry: &PublicKeyRegistry,
    validator_order: &[String],
) -> ChainIntegrityReport {
    let mut issues = Vec::new();
    let mut replayed = initial_state.clone();
    let mut seen = HashSet::new();

    if chain.is_empty() {
        push_issue(&mut issues, None, "missing_genesis", "цепочка пуста");
    } else {
        let genesis = &chain[0];
        let expected_genesis = genesis_block(initial_state);
        if genesis.header != expected_genesis.header {
            push_issue(
                &mut issues,
                Some(0),
                "genesis_header",
                "заголовок генезис-блока не соответствует конфигурации",
            );
        }
        if genesis.recompute_hash() != genesis.hash {
            push_issue(
                &mut issues,
                Some(0),
                "block_hash",
                "хеш генезис-блока не соответствует содержимому",
            );
        }
    }

    for index in 1..chain.len() {
        let block = &chain[index];
        let height = Some(block.header.height);
        if block.header.height != index as u64 {
            push_issue(
                &mut issues,
                height,
                "height",
                format!("ожидалась высота {index}"),
            );
        }

        let recomputed_hash = block.recompute_hash();
        if recomputed_hash != block.hash {
            push_issue(
                &mut issues,
                height,
                "block_hash",
                "изменение хеша блока: сохранённый хеш не соответствует содержимому",
            );
        }

        let recomputed_previous = chain[index - 1].recompute_hash();
        if block.header.previous_hash != recomputed_previous {
            push_issue(
                &mut issues,
                height,
                "previous_hash",
                "нарушена хеш-связь с пересчитанным предыдущим блоком",
            );
        }

        // Для аудита локального хранилища корень строится из повторно
        // вычисленных идентификаторов. Иначе изменение тела транзакции при
        // сохранённом старом `id` обнаружилось бы проверкой id, но не корнем.
        let ids = block
            .transactions
            .iter()
            .map(Transaction::recompute_id)
            .collect::<Vec<_>>();
        match merkle_root_from_ids(&ids) {
            Ok(root) if root != block.header.transactions_root => push_issue(
                &mut issues,
                height,
                "transactions_root",
                "корень транзакций не соответствует телу блока",
            ),
            Err(error) => push_issue(
                &mut issues,
                height,
                "transactions_root",
                format!("невозможно вычислить корень транзакций: {error}"),
            ),
            _ => {}
        }

        if validator_order.is_empty() {
            push_issue(
                &mut issues,
                height,
                "validator_order",
                "реестр валидаторов пуст",
            );
        } else {
            let expected = &validator_order[block.header.round as usize % validator_order.len()];
            if &block.header.proposer != expected {
                push_issue(
                    &mut issues,
                    height,
                    "proposer_round",
                    format!("ожидался автор {expected}"),
                );
            }
        }

        match registry.get(&block.header.proposer) {
            Some(public_key) => match decode_fixed::<32>(&block.hash) {
                Ok(hash) => {
                    if verify_hex(public_key, &hash, &block.proposer_signature).is_err() {
                        push_issue(
                            &mut issues,
                            height,
                            "proposer_signature",
                            "некорректная подпись автора блока",
                        );
                    }
                }
                Err(_) => push_issue(
                    &mut issues,
                    height,
                    "block_hash_format",
                    "хеш блока имеет некорректный формат",
                ),
            },
            None => push_issue(
                &mut issues,
                height,
                "proposer_authority",
                "автор блока не зарегистрирован",
            ),
        }

        for transaction in &block.transactions {
            if let Err(error) =
                validate_and_apply_transaction(transaction, &mut replayed, &mut seen)
            {
                push_issue(
                    &mut issues,
                    height,
                    error.code(),
                    format!("транзакция {}: {error}", transaction.id),
                );
            }
        }

        let computed_state_hash = state_hash(&replayed);
        if computed_state_hash != block.header.state_hash {
            push_issue(
                &mut issues,
                height,
                "state_hash",
                "state_hash не соответствует повторному исполнению цепочки",
            );
        }

        if let Err(error) = validate_votes(block, registry, true) {
            push_issue(
                &mut issues,
                height,
                error.code(),
                format!("проверка голосов: {error}"),
            );
        }
    }

    let replayed_state_hash = state_hash(&replayed);
    let stored_state_hash = state_hash(stored_state);
    if replayed_state_hash != stored_state_hash {
        push_issue(
            &mut issues,
            None,
            "node_state_mismatch",
            "состояние узла не соответствует состоянию, повторно вычисленному из его цепочки",
        );
    }

    ChainIntegrityReport {
        node_id: node_id.to_string(),
        node_name: node_name.to_string(),
        valid: issues.is_empty(),
        checked_blocks: chain.len(),
        stored_state_hash,
        replayed_state_hash,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::{crypto::public_key_hex, model::Transaction};

    fn state_and_keys() -> (LedgerState, SigningKey, SigningKey) {
        let bair = SigningKey::from_bytes(&[11; 32]);
        let bato = SigningKey::from_bytes(&[12; 32]);
        let mut state = LedgerState::new();
        state.insert(
            public_key_hex(&bair),
            Account {
                address: public_key_hex(&bair),
                label: "Баир".into(),
                balance: 100,
                nonce: 0,
            },
        );
        state.insert(
            public_key_hex(&bato),
            Account {
                address: public_key_hex(&bato),
                label: "Бато".into(),
                balance: 0,
                nonce: 0,
            },
        );
        (state, bair, bato)
    }

    #[test]
    fn valid_transaction_changes_balances_and_nonce() {
        let (mut state, bair, bato) = state_and_keys();
        let transaction = Transaction::new_signed(&bair, public_key_hex(&bato), 25, 0);
        let mut seen = HashSet::new();
        validate_and_apply_transaction(&transaction, &mut state, &mut seen).unwrap();
        assert_eq!(state[&public_key_hex(&bair)].balance, 75);
        assert_eq!(state[&public_key_hex(&bair)].nonce, 1);
        assert_eq!(state[&public_key_hex(&bato)].balance, 25);
    }

    #[test]
    fn insufficient_balance_is_rejected() {
        let (state, bair, bato) = state_and_keys();
        let transaction = Transaction::new_signed(&bair, public_key_hex(&bato), 101, 0);
        assert!(matches!(
            validate_transaction(&transaction, &state, &HashSet::new()),
            Err(ValidationError::InsufficientBalance { .. })
        ));
    }

    #[test]
    fn wrong_nonce_is_rejected() {
        let (state, bair, bato) = state_and_keys();
        let transaction = Transaction::new_signed(&bair, public_key_hex(&bato), 1, 7);
        assert!(matches!(
            validate_transaction(&transaction, &state, &HashSet::new()),
            Err(ValidationError::Nonce { .. })
        ));
    }

    #[test]
    fn changed_signed_data_is_rejected() {
        let (state, bair, bato) = state_and_keys();
        let mut transaction = Transaction::new_signed(&bair, public_key_hex(&bato), 1, 0);
        transaction.amount = 2;
        assert!(matches!(
            validate_transaction(&transaction, &state, &HashSet::new()),
            Err(ValidationError::TransactionId)
        ));
    }

    #[test]
    fn recomputed_id_does_not_make_stale_signature_valid() {
        let (state, bair, bato) = state_and_keys();
        let mut transaction = Transaction::new_signed(&bair, public_key_hex(&bato), 1, 0);
        transaction.amount = 2;
        transaction.id = transaction.recompute_id();
        assert!(matches!(
            validate_transaction(&transaction, &state, &HashSet::new()),
            Err(ValidationError::TransactionSignature)
        ));
    }

    #[test]
    fn state_hash_ignores_insertion_order() {
        let (state, _, _) = state_and_keys();
        let reversed = state
            .iter()
            .rev()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(state_hash(&state), state_hash(&reversed));
    }
}
