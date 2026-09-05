use crate::{
    model::ScenarioReport,
    network::{Network, NetworkError},
};

impl Network {
    pub fn run_scenario(&mut self, name: &str) -> Result<ScenarioReport, NetworkError> {
        match name {
            "1" | "valid_transaction" | "valid-transaction" => self.scenario_valid_transaction(),
            "2" | "invalid_signature" | "invalid-signature" => self.scenario_invalid_signature(),
            "3" | "insufficient_balance" | "insufficient-balance" => {
                self.scenario_insufficient_balance()
            }
            "4" | "replay_transaction" | "replay-transaction" => self.scenario_replay_transaction(),
            "5" | "one_validator_offline" | "one-validator-offline" => {
                self.scenario_one_validator_offline()
            }
            "6" | "two_validators_offline" | "two-validators-offline" => {
                self.scenario_two_validators_offline()
            }
            "7" | "invalid_block" | "invalid-block" => self.scenario_invalid_block(),
            "8" | "tamper_saved_block" | "tamper-saved-block" => self.scenario_tamper_saved_block(),
            other => Err(NetworkError::Demo(format!(
                "неизвестный сценарий '{other}'; допустимы 1–8 или их символьные имена"
            ))),
        }
    }

    fn report(
        &self,
        name: &str,
        title: &str,
        passed: bool,
        summary: impl Into<String>,
        observations: Vec<String>,
    ) -> ScenarioReport {
        ScenarioReport {
            name: name.to_string(),
            title: title.to_string(),
            passed,
            summary: summary.into(),
            observations,
            snapshot: self.snapshot(),
            integrity: self.integrity(),
        }
    }

    fn scenario_valid_transaction(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        let before = self.snapshot();
        let bair_before = before
            .accounts
            .iter()
            .find(|account| account.label == "Баир")
            .expect("учебный счёт «Баир» должен существовать")
            .balance;
        let transaction = self.create_and_submit_transaction("Баир", "Бато", 40, None)?;
        let attempt = self.produce_block()?;
        let after = self.snapshot();
        let bair_after = after
            .accounts
            .iter()
            .find(|account| account.label == "Баир")
            .expect("учебный счёт «Баир» должен существовать");
        let bato_after = after
            .accounts
            .iter()
            .find(|account| account.label == "Бато")
            .expect("учебный счёт «Бато» должен существовать");
        let passed = attempt.confirmed
            && attempt.votes.len() >= 3
            && bair_after.balance == bair_before - 40
            && bair_after.nonce == 1;
        Ok(self.report(
            "valid_transaction",
            "Сценарий 1. Корректная транзакция",
            passed,
            if passed {
                "Транзакция прошла проверку, получила кворум и изменила согласованное состояние"
            } else {
                "Ожидаемый переход состояния не выполнен"
            },
            vec![
                format!("Создана и подписана транзакция {}", transaction.id),
                format!("Получено голосов: {}; требуется: 3", attempt.votes.len()),
                format!(
                    "Баланс счёта «Баир»: {bair_before} → {}",
                    bair_after.balance
                ),
                format!("Баланс счёта «Бато» после блока: {}", bato_after.balance),
                format!("Nonce счёта «Баир» после блока: {}", bair_after.nonce),
            ],
        ))
    }

    fn scenario_invalid_signature(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        let mut transaction = self.make_signed_transaction("Баир", "Бато", 10, None)?;
        transaction.amount = 11;
        transaction.id = transaction.recompute_id();
        let result = self.submit_transaction(transaction);
        let error = result
            .as_ref()
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "ошибка отсутствует".to_string());
        let passed = result.is_err() && self.snapshot().mempool.is_empty();
        Ok(self.report(
            "invalid_signature",
            "Сценарий 2. Некорректная подпись",
            passed,
            "После подписания сумма была изменена; транзакция отклонена до попадания в пул",
            vec![
                "Подписано исходное каноническое представление с amount=10".to_string(),
                "После подписи поле amount изменено на 11, а id пересчитан без создания новой подписи"
                    .to_string(),
                format!("Результат проверки: {error}"),
            ],
        ))
    }

    fn scenario_insufficient_balance(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        let bair_balance = self
            .snapshot()
            .accounts
            .iter()
            .find(|account| account.label == "Баир")
            .expect("учебный счёт «Баир» должен существовать")
            .balance;
        let transaction =
            self.make_signed_transaction("Баир", "Бато", bair_balance.saturating_add(1), None)?;
        let result = self.submit_transaction(transaction);
        let error = result
            .as_ref()
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "ошибка отсутствует".to_string());
        let passed = result.is_err() && self.snapshot().mempool.is_empty();
        Ok(self.report(
            "insufficient_balance",
            "Сценарий 3. Недостаточный баланс",
            passed,
            "Корректно подписанная транзакция отклонена семантической проверкой баланса",
            vec![
                format!("Доступный баланс счёта «Баир»: {bair_balance}"),
                format!("Запрошенная сумма: {}", bair_balance + 1),
                format!("Результат проверки: {error}"),
            ],
        ))
    }

    fn scenario_replay_transaction(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        let transaction = self.create_and_submit_transaction("Баир", "Бато", 5, None)?;
        let first = self.produce_block()?;
        let replay = self.submit_transaction(transaction.clone());
        let error = replay
            .as_ref()
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "ошибка отсутствует".to_string());
        let passed = first.confirmed && replay.is_err() && self.snapshot().mempool.is_empty();
        Ok(self.report(
            "replay_transaction",
            "Сценарий 4. Повторная транзакция",
            passed,
            "Повтор ранее подтверждённой транзакции не принят в пул",
            vec![
                format!("Первичное подтверждение: {}", first.confirmed),
                format!(
                    "Повторно отправлен id {} с nonce {}",
                    transaction.id, transaction.nonce
                ),
                format!("Результат повторной проверки: {error}"),
            ],
        ))
    }

    fn scenario_one_validator_offline(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        self.set_validator_active("validator-4", false)?;
        self.create_and_submit_transaction("Баир", "Бато", 10, None)?;
        let attempt = self.produce_block()?;
        let passed = attempt.confirmed && attempt.votes.len() == 3;
        Ok(self.report(
            "one_validator_offline",
            "Сценарий 5. Один валидатор отключён",
            passed,
            "Три активных валидатора образовали требуемый кворум 3 из 4",
            vec![
                "Валидатор 4 отключён до формирования блока".to_string(),
                format!(
                    "Число активных валидаторов: {}",
                    self.active_validator_count()
                ),
                format!("Получено голосов: {}", attempt.votes.len()),
                format!("Блок подтверждён: {}", attempt.confirmed),
            ],
        ))
    }

    fn scenario_two_validators_offline(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        self.set_validator_active("validator-3", false)?;
        self.set_validator_active("validator-4", false)?;
        self.create_and_submit_transaction("Баир", "Бато", 10, None)?;
        let attempt = self.produce_block()?;
        let passed = !attempt.confirmed && attempt.votes.len() == 2;
        Ok(self.report(
            "two_validators_offline",
            "Сценарий 6. Два валидатора отключены",
            passed,
            "Два голоса не образуют кворум; кандидат не добавлен в цепочки",
            vec![
                "Валидаторы 3 и 4 отключены".to_string(),
                format!(
                    "Число активных валидаторов: {}",
                    self.active_validator_count()
                ),
                format!("Получено голосов: {}", attempt.votes.len()),
                format!("Причина: {}", attempt.reason),
            ],
        ))
    }

    fn scenario_invalid_block(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        self.create_and_submit_transaction("Баир", "Бато", 10, None)?;
        let attempt = self.demo_invalid_block("previous_hash")?;
        let rejected = attempt
            .validations
            .iter()
            .filter(|validation| validation.active)
            .all(|validation| !validation.accepted);
        let passed = !attempt.confirmed && attempt.votes.is_empty() && rejected;
        Ok(self.report(
            "invalid_block",
            "Сценарий 7. Некорректный блок",
            passed,
            "Кандидат с подменённым previous_hash независимо отклонён всеми активными валидаторами",
            vec![
                "После подписи кандидата previous_hash заменён демонстрационной функцией"
                    .to_string(),
                format!("Активные валидаторы единогласно отклонили: {rejected}"),
                format!("Получено положительных голосов: {}", attempt.votes.len()),
                format!("Блок подтверждён: {}", attempt.confirmed),
            ],
        ))
    }

    fn scenario_tamper_saved_block(&mut self) -> Result<ScenarioReport, NetworkError> {
        self.reset();
        self.create_and_submit_transaction("Баир", "Бато", 10, None)?;
        let first = self.produce_block()?;
        self.create_and_submit_transaction("Баир", "Аюна", 7, None)?;
        let second = self.produce_block()?;
        if !first.confirmed || !second.confirmed {
            return Err(NetworkError::Demo(
                "не удалось подготовить два подтверждённых блока".to_string(),
            ));
        }
        self.demo_tamper_transaction("validator-1", 1, 0)?;
        let integrity = self.integrity();
        let validator_one = integrity
            .reports
            .iter()
            .find(|report| report.node_id == "validator-1")
            .expect("validator-1 report");
        let codes = validator_one
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>();
        let passed = !integrity.valid
            && !integrity.replicas_consistent
            && codes.contains(&"block_hash")
            && codes.contains(&"transactions_root")
            && codes.contains(&"previous_hash")
            && (codes.contains(&"state_hash") || codes.contains(&"node_state_mismatch"));
        Ok(self.report(
            "tamper_saved_block",
            "Сценарий 8. Изменение сохранённого блока",
            passed,
            "Подмена только одной локальной копии выявлена повторным вычислением хешей, корня, состояния и сравнением реплик",
            vec![
                "Сначала созданы два подтверждённых блока для проверки последующей ссылки".to_string(),
                "В локальной копии валидатора 1 изменена сумма первой транзакции блока высоты 1"
                    .to_string(),
                format!("Коды обнаруженных несоответствий: {}", codes.join(", ")),
                format!("Реплики согласованы: {}", integrity.replicas_consistent),
                "Валидаторы 2–4 сохранили исходные неизменённые копии".to_string(),
            ],
        ))
    }
}
