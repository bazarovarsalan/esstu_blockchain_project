use round_robin_quorum::{Network, ValidationError, ledger::validate_candidate};

#[test]
fn all_eight_demonstration_scenarios_reach_expected_outcome() {
    let mut network = Network::new();
    let scenarios = [
        "valid_transaction",
        "invalid_signature",
        "insufficient_balance",
        "replay_transaction",
        "one_validator_offline",
        "two_validators_offline",
        "invalid_block",
        "tamper_saved_block",
    ];
    for scenario in scenarios {
        let report = network.run_scenario(scenario).unwrap();
        assert!(
            report.passed,
            "scenario failed: {scenario}: {}",
            report.summary
        );
    }
}

#[test]
fn active_replicas_commit_identical_chains_and_state() {
    let mut network = Network::new();
    network.set_validator_active("validator-4", false).unwrap();
    network
        .create_and_submit_transaction("Баир", "Бато", 20, None)
        .unwrap();
    assert!(network.produce_block().unwrap().confirmed);

    let active = network
        .nodes
        .iter()
        .filter(|node| node.active)
        .collect::<Vec<_>>();
    for node in active.iter().skip(1) {
        assert_eq!(node.chain, active[0].chain);
        assert_eq!(node.state, active[0].state);
    }
    assert!(network.integrity().replicas_consistent);
}

#[test]
fn buryat_account_labels_are_resolved_without_case_sensitivity() {
    let mut network = Network::new();
    network
        .create_and_submit_transaction("баир", "бато", 1, None)
        .unwrap();
    assert_eq!(network.snapshot().mempool.len(), 1);
}

#[test]
fn transaction_root_tamper_is_found_by_chain_audit() {
    let mut network = Network::new();
    network
        .create_and_submit_transaction("Баир", "Бато", 20, None)
        .unwrap();
    assert!(network.produce_block().unwrap().confirmed);
    network.nodes[0].chain[1].header.transactions_root = "ff".repeat(32);
    let integrity = network.integrity();
    let report = &integrity.reports[0];
    assert!(!report.valid);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "transactions_root")
    );
}

#[test]
fn state_hash_tamper_is_found_by_chain_audit() {
    let mut network = Network::new();
    network
        .create_and_submit_transaction("Баир", "Бато", 20, None)
        .unwrap();
    assert!(network.produce_block().unwrap().confirmed);
    network.nodes[0].chain[1].header.state_hash = "ff".repeat(32);
    let integrity = network.integrity();
    assert!(
        integrity.reports[0]
            .issues
            .iter()
            .any(|issue| issue.code == "state_hash")
    );
}

#[test]
fn previous_hash_tamper_is_rejected_during_candidate_validation() {
    let mut network = Network::new();
    network
        .create_and_submit_transaction("Баир", "Бато", 20, None)
        .unwrap();
    let mut candidate = network.build_candidate().unwrap();
    candidate.header.previous_hash = "ff".repeat(32);
    let node = &network.nodes[0];
    let error = validate_candidate(
        &candidate,
        &node.chain,
        &node.state,
        network.registry(),
        network.current_proposer_id(),
    )
    .unwrap_err();
    assert!(matches!(error, ValidationError::PreviousHash));
}

#[test]
fn tampering_one_old_block_breaks_next_link_and_replica_consistency() {
    let mut network = Network::new();
    let report = network.run_scenario("tamper_saved_block").unwrap();
    assert!(report.passed);
    assert!(!report.integrity.replicas_consistent);
    let first = report
        .integrity
        .reports
        .iter()
        .find(|report| report.node_id == "validator-1")
        .unwrap();
    assert!(first.issues.iter().any(|issue| issue.code == "block_hash"));
    assert!(
        first
            .issues
            .iter()
            .any(|issue| issue.code == "previous_hash")
    );
    assert!(
        first
            .issues
            .iter()
            .any(|issue| issue.code == "node_state_mismatch")
    );
}

#[test]
fn reenabled_validator_synchronizes_before_voting() {
    let mut network = Network::new();
    network.set_validator_active("validator-4", false).unwrap();
    network
        .create_and_submit_transaction("Баир", "Бато", 3, None)
        .unwrap();
    assert!(network.produce_block().unwrap().confirmed);
    assert_eq!(network.nodes[3].chain.len(), 1);
    network.set_validator_active("validator-4", true).unwrap();
    assert_eq!(network.nodes[3].chain, network.nodes[0].chain);
    assert_eq!(network.nodes[3].state, network.nodes[0].state);
}
