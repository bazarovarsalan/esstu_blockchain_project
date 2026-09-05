export interface Account {
  address: string;
  label: string;
  balance: number;
  nonce: number;
}

export interface Transaction {
  id: string;
  sender: string;
  recipient: string;
  amount: number;
  nonce: number;
  signature: string;
}

export interface Vote {
  validator_id: string;
  block_hash: string;
  round: number;
  approve: boolean;
  signature: string;
}

export interface CheckResult {
  name: string;
  status: "passed" | "failed" | "not_reached";
  detail: string;
}

export interface ValidatorValidation {
  validator_id: string;
  validator_name: string;
  active: boolean;
  accepted: boolean;
  checks: CheckResult[];
  error: string | null;
}

export interface ConsensusAttempt {
  round: number;
  proposer_id: string;
  proposer_name: string;
  block_hash: string | null;
  validations: ValidatorValidation[];
  votes: Vote[];
  quorum_required: number;
  quorum_reached: boolean;
  confirmed: boolean;
  reason: string;
}

export interface ValidatorSummary {
  id: string;
  name: string;
  public_key: string;
  active: boolean;
  chain_height: number;
  last_hash: string;
  mempool_size: number;
  state_hash: string;
}

export interface BlockSummary {
  height: number;
  hash: string;
  previous_hash: string;
  transaction_count: number;
  proposer: string;
  round: number;
  vote_count: number;
  timestamp: number;
}

export interface Block {
  hash: string;
  header: {
    height: number;
    previous_hash: string;
    transactions_root: string;
    state_hash: string;
    timestamp: number;
    round: number;
    proposer: string;
  };
  transactions: Transaction[];
  proposer_signature: string;
  votes: Vote[];
}

export interface EventRecord {
  sequence: number;
  timestamp_ms: number;
  node_id: string | null;
  level: string;
  kind: string;
  message: string;
}

export interface NetworkSnapshot {
  protocol: string;
  simulation_notice: string;
  current_round: number;
  current_proposer_id: string;
  current_proposer_name: string;
  quorum_required: number;
  validators: ValidatorSummary[];
  accounts: Account[];
  mempool: Transaction[];
  blocks: BlockSummary[];
  last_consensus: ConsensusAttempt | null;
  events: EventRecord[];
}

export interface ChainIssue {
  height: number | null;
  code: string;
  message: string;
}

export interface ChainIntegrityReport {
  node_id: string;
  node_name: string;
  valid: boolean;
  checked_blocks: number;
  stored_state_hash: string;
  replayed_state_hash: string;
  issues: ChainIssue[];
}

export interface IntegrityOverview {
  valid: boolean;
  replicas_consistent: boolean;
  reports: ChainIntegrityReport[];
  consistency_notes: string[];
}

export interface ScenarioReport {
  name: string;
  title: string;
  passed: boolean;
  summary: string;
  observations: string[];
  snapshot: NetworkSnapshot;
  integrity: IntegrityOverview;
}

