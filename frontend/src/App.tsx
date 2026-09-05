import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import type {
  Block,
  IntegrityOverview,
  NetworkSnapshot,
  ScenarioReport,
} from "./types";

const scenarios = [
  ["valid_transaction", "1. Корректная транзакция"],
  ["invalid_signature", "2. Некорректная подпись"],
  ["insufficient_balance", "3. Недостаточный баланс"],
  ["replay_transaction", "4. Повторная транзакция"],
  ["one_validator_offline", "5. Один валидатор отключён"],
  ["two_validators_offline", "6. Два валидатора отключены"],
  ["invalid_block", "7. Некорректный блок"],
  ["tamper_saved_block", "8. Изменение сохранённого блока"],
] as const;

function short(value: string, length = 12) {
  if (!value) return "—";
  return value.length <= length * 2 + 1
    ? value
    : `${value.slice(0, length)}…${value.slice(-length)}`;
}

function accountName(snapshot: NetworkSnapshot, address: string) {
  return snapshot.accounts.find((account) => account.address === address)?.label ?? short(address, 6);
}

function App() {
  const [snapshot, setSnapshot] = useState<NetworkSnapshot | null>(null);
  const [integrity, setIntegrity] = useState<IntegrityOverview | null>(null);
  const [selectedHeight, setSelectedHeight] = useState<number | null>(null);
  const [selectedBlock, setSelectedBlock] = useState<Block | null>(null);
  const [scenario, setScenario] = useState<ScenarioReport | null>(null);
  const [sender, setSender] = useState("Баир");
  const [recipient, setRecipient] = useState("Бато");
  const [amount, setAmount] = useState(25);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async (silent = false) => {
    try {
      const state = await api.state();
      setSnapshot(state);
      if (!silent) setError(null);
    } catch (requestError) {
      if (!silent) {
        setError(requestError instanceof Error ? requestError.message : String(requestError));
      }
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(true), 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!snapshot?.blocks.length) return;
    const exists = snapshot.blocks.some((block) => block.height === selectedHeight);
    if (!exists) setSelectedHeight(snapshot.blocks.at(-1)!.height);
  }, [selectedHeight, snapshot?.blocks]);

  useEffect(() => {
    if (selectedHeight === null) return;
    api.block(selectedHeight).then(setSelectedBlock).catch(() => setSelectedBlock(null));
  }, [selectedHeight, snapshot?.blocks]);

  async function perform(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      await refresh(true);
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : String(requestError));
    } finally {
      setBusy(false);
    }
  }

  function submitTransaction(event: FormEvent) {
    event.preventDefault();
    void perform(async () => {
      const transaction = await api.createTransaction({ sender, recipient, amount });
      setNotice(`Транзакция ${short(transaction.id)} подписана и принята в пул.`);
    });
  }

  const lifecycle = useMemo(() => {
    const attempt = snapshot?.last_consensus;
    const hasPending = Boolean(snapshot?.mempool.length);
    const reachedBlock = Boolean(attempt?.block_hash);
    return [
      ["Создание", hasPending || reachedBlock ? "done" : "idle"],
      ["Подпись", hasPending || reachedBlock ? "done" : "idle"],
      ["Проверка и пул", hasPending || reachedBlock ? "done" : "idle"],
      ["Формирование блока", reachedBlock ? "done" : "idle"],
      ["Голосование", attempt ? (attempt.votes.length ? "done" : "failed") : "idle"],
      ["Кворум и фиксация", attempt ? (attempt.confirmed ? "done" : "failed") : "idle"],
    ] as const;
  }, [snapshot]);

  if (!snapshot) {
    return (
      <main className="loading-page">
        <h1>RoundRobinQuorum</h1>
        <p>{error ?? "Подключение к Rust API…"}</p>
        <button onClick={() => void refresh()} type="button">
          Повторить
        </button>
      </main>
    );
  }

  return (
    <main className="app-shell">
      <header className="hero">
        <div>
          <p className="eyebrow">Учебно-исследовательский прототип</p>
          <h1>RoundRobinQuorum</h1>
          <p className="subtitle">Консорциумная сеть · SHA-256 · Ed25519 · кворум 3 из 4</p>
        </div>
        <div className="round-card">
          <span>Текущий раунд</span>
          <strong>{snapshot.current_round}</strong>
          <small>Автор: {snapshot.current_proposer_name}</small>
        </div>
      </header>

      <div className="simulation-notice">{snapshot.simulation_notice}</div>
      {error && <div className="alert error">{error}</div>}
      {notice && <div className="alert success">{notice}</div>}

      <section className="panel">
        <div className="section-heading">
          <div>
            <p className="eyebrow">Состояние сети</p>
            <h2>Четыре независимые реплики</h2>
          </div>
          <span className="quorum-label">Требуемый кворум: {snapshot.quorum_required}/4</span>
        </div>
        <div className="validator-grid">
          {snapshot.validators.map((validator) => (
            <article className={`validator-card ${validator.active ? "online" : "offline"}`} key={validator.id}>
              <div className="validator-title">
                <div>
                  <span className="status-dot" />
                  <strong>{validator.name}</strong>
                </div>
                <button
                  className="toggle"
                  disabled={busy}
                  onClick={() =>
                    void perform(async () => {
                      await api.validatorStatus(validator.id, !validator.active);
                    })
                  }
                  type="button"
                >
                  {validator.active ? "Отключить" : "Включить"}
                </button>
              </div>
              <dl className="metric-list">
                <div><dt>Статус</dt><dd>{validator.active ? "активен" : "отключён"}</dd></div>
                <div><dt>Высота</dt><dd>{validator.chain_height}</dd></div>
                <div><dt>Пул</dt><dd>{validator.mempool_size}</dd></div>
              </dl>
              <code title={validator.last_hash}>last: {short(validator.last_hash, 7)}</code>
              <code title={validator.state_hash}>state: {short(validator.state_hash, 7)}</code>
            </article>
          ))}
        </div>
      </section>

      <section className="two-column">
        <div className="panel">
          <div className="section-heading">
            <div><p className="eyebrow">Шаг 1</p><h2>Создать транзакцию</h2></div>
          </div>
          <form className="transaction-form" onSubmit={submitTransaction}>
            <label>
              Отправитель
              <select value={sender} onChange={(event) => setSender(event.target.value)}>
                {snapshot.accounts.map((account) => <option key={account.address}>{account.label}</option>)}
              </select>
            </label>
            <label>
              Получатель
              <select value={recipient} onChange={(event) => setRecipient(event.target.value)}>
                {snapshot.accounts.map((account) => <option key={account.address}>{account.label}</option>)}
              </select>
            </label>
            <label>
              Сумма
              <input min="1" onChange={(event) => setAmount(Number(event.target.value))} type="number" value={amount} />
            </label>
            <button className="primary" disabled={busy || sender === recipient || amount < 1} type="submit">
              Подписать и отправить
            </button>
          </form>
          <p className="explanation">Закрытые ключи учебных счетов находятся в памяти backend. В реальной системе транзакцию подписывает внешний кошелёк.</p>
        </div>

        <div className="panel">
          <div className="section-heading">
            <div><p className="eyebrow">Состояние</p><h2>Счета и балансы</h2></div>
          </div>
          <div className="table-wrap">
            <table>
              <thead><tr><th>Счёт</th><th>Баланс</th><th>Следующий nonce</th></tr></thead>
              <tbody>
                {snapshot.accounts.map((account) => (
                  <tr key={account.address}>
                    <td><strong>{account.label}</strong><code title={account.address}>{short(account.address, 6)}</code></td>
                    <td>{account.balance.toLocaleString("ru-RU")}</td>
                    <td>{account.nonce}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="section-heading">
          <div><p className="eyebrow">Наблюдаемый конвейер</p><h2>От транзакции до подтверждённого блока</h2></div>
          <button
            className="primary"
            disabled={busy}
            onClick={() => void perform(async () => {
              const attempt = await api.produceBlock();
              setNotice(attempt.reason);
            })}
            type="button"
          >
            Сформировать блок
          </button>
        </div>
        <div className="lifecycle">
          {lifecycle.map(([label, status], index) => (
            <div className={`lifecycle-step ${status}`} key={label}>
              <span>{index + 1}</span><strong>{label}</strong>
            </div>
          ))}
        </div>
        <div className="mempool">
          <h3>Пул неподтверждённых транзакций ({snapshot.mempool.length})</h3>
          {snapshot.mempool.length === 0 ? <p className="empty">Пул пуст.</p> : snapshot.mempool.map((transaction) => (
            <article className="transaction-row" key={transaction.id}>
              <div><strong>{accountName(snapshot, transaction.sender)} → {accountName(snapshot, transaction.recipient)}</strong><span>{transaction.amount} ед. · nonce {transaction.nonce}</span></div>
              <code title={transaction.id}>id {short(transaction.id, 8)}</code>
              <code title={transaction.signature}>signature {short(transaction.signature, 8)}</code>
            </article>
          ))}
        </div>
      </section>

      {snapshot.last_consensus && (
        <section className="panel">
          <div className="section-heading">
            <div><p className="eyebrow">Раунд {snapshot.last_consensus.round}</p><h2>Независимая проверка и голоса</h2></div>
            <span className={`result-badge ${snapshot.last_consensus.confirmed ? "passed" : "failed"}`}>
              {snapshot.last_consensus.confirmed ? "Кворум достигнут" : "Кворум отсутствует"}
            </span>
          </div>
          <p>{snapshot.last_consensus.reason}</p>
          <div className="validation-grid">
            {snapshot.last_consensus.validations.map((validation) => (
              <details className={`validation-card ${validation.accepted ? "accepted" : "rejected"}`} key={validation.validator_id}>
                <summary>
                  <strong>{validation.validator_name}</strong>
                  <span>{!validation.active ? "не участвовал" : validation.accepted ? "голос «за»" : "отклонено"}</span>
                </summary>
                {validation.error && <p className="validation-error">{validation.error}</p>}
                <ol className="checks">
                  {validation.checks.map((check) => <li className={check.status} key={check.name}>{check.detail}</li>)}
                </ol>
              </details>
            ))}
          </div>
          <div className="votes">
            <h3>Подписанные голоса: {snapshot.last_consensus.votes.length}/4</h3>
            {snapshot.last_consensus.votes.map((vote) => (
              <div className="vote" key={vote.validator_id}><span>{vote.validator_id}</span><code title={vote.signature}>{short(vote.signature, 8)}</code></div>
            ))}
          </div>
        </section>
      )}

      <section className="panel">
        <div className="section-heading">
          <div><p className="eyebrow">Воспроизводимые проверки</p><h2>Восемь демонстрационных сценариев</h2></div>
          <button className="secondary" disabled={busy} onClick={() => void perform(async () => { setSnapshot(await api.reset()); setScenario(null); setIntegrity(null); })} type="button">Сбросить сеть</button>
        </div>
        <div className="scenario-grid">
          {scenarios.map(([name, title]) => (
            <button disabled={busy} key={name} onClick={() => void perform(async () => {
              const report = await api.scenario(name);
              setScenario(report);
              setSnapshot(report.snapshot);
              setIntegrity(report.integrity);
              setNotice(`${report.passed ? "Пройден" : "Не пройден"}: ${report.title}`);
            })} type="button">{title}</button>
          ))}
        </div>
        {scenario && (
          <article className={`scenario-report ${scenario.passed ? "passed" : "failed"}`}>
            <div><strong>{scenario.title}</strong><span>{scenario.passed ? "ожидаемый результат получен" : "есть расхождение"}</span></div>
            <p>{scenario.summary}</p>
            <ul>{scenario.observations.map((observation) => <li key={observation}>{observation}</li>)}</ul>
          </article>
        )}
      </section>

      <section className="two-column wide-left">
        <div className="panel">
          <div className="section-heading"><div><p className="eyebrow">Реестр</p><h2>Обозреватель блоков</h2></div></div>
          <div className="block-browser">
            <nav>
              {snapshot.blocks.map((block) => (
                <button className={selectedHeight === block.height ? "selected" : ""} key={block.height} onClick={() => setSelectedHeight(block.height)} type="button">
                  <strong>Блок #{block.height}</strong><span>{block.transaction_count} транз. · {block.vote_count} голос.</span><code>{short(block.hash, 6)}</code>
                </button>
              ))}
            </nav>
            {selectedBlock && (
              <article className="block-detail">
                <h3>Блок #{selectedBlock.header.height}</h3>
                <dl className="hash-list">
                  <div><dt>hash</dt><dd><code>{selectedBlock.hash}</code></dd></div>
                  <div><dt>previous_hash</dt><dd><code>{selectedBlock.header.previous_hash}</code></dd></div>
                  <div><dt>transactions_root</dt><dd><code>{selectedBlock.header.transactions_root}</code></dd></div>
                  <div><dt>state_hash</dt><dd><code>{selectedBlock.header.state_hash}</code></dd></div>
                  <div><dt>round / proposer</dt><dd>{selectedBlock.header.round} / {selectedBlock.header.proposer}</dd></div>
                </dl>
                <h4>Транзакции ({selectedBlock.transactions.length})</h4>
                {selectedBlock.transactions.length === 0 ? <p className="empty">Генезис-блок не содержит транзакций.</p> : selectedBlock.transactions.map((transaction) => (
                  <div className="block-transaction" key={transaction.id}><strong>{accountName(snapshot, transaction.sender)} → {accountName(snapshot, transaction.recipient)}: {transaction.amount}</strong><code>{transaction.id}</code></div>
                ))}
              </article>
            )}
          </div>
        </div>

        <div className="panel">
          <div className="section-heading">
            <div><p className="eyebrow">Аудит</p><h2>Целостность цепочек</h2></div>
            <button className="secondary" disabled={busy} onClick={() => void perform(async () => setIntegrity(await api.integrity()))} type="button">Проверить</button>
          </div>
          {!integrity ? <p className="empty">Запустите повторную проверку всех локальных копий.</p> : (
            <>
              <div className={`integrity-overall ${integrity.valid ? "valid" : "invalid"}`}>
                <strong>{integrity.valid ? "Цепочки корректны" : "Обнаружено несоответствие"}</strong>
                <span>Реплики {integrity.replicas_consistent ? "согласованы" : "различаются"}</span>
              </div>
              {integrity.reports.map((report) => (
                <details className="integrity-node" key={report.node_id} open={!report.valid}>
                  <summary>{report.node_name}<span>{report.valid ? "корректна" : `${report.issues.length} ошибок`}</span></summary>
                  {report.issues.map((issue, index) => <p key={`${issue.code}-${index}`}><code>{issue.code}</code> {issue.message}</p>)}
                </details>
              ))}
              {integrity.consistency_notes.map((note) => <p className="consistency-note" key={note}>{note}</p>)}
            </>
          )}
        </div>
      </section>

      <section className="panel event-panel">
        <div className="section-heading"><div><p className="eyebrow">Наблюдаемость</p><h2>Журнал событий системы</h2></div><span>{snapshot.events.length} записей</span></div>
        <div className="event-log">
          {[...snapshot.events].reverse().map((event) => (
            <div className={`event ${event.level}`} key={event.sequence}>
              <time>{new Date(event.timestamp_ms).toLocaleTimeString("ru-RU")}</time>
              <code>{event.kind}</code>
              <span>{event.node_id && `[${event.node_id}] `}{event.message}</span>
            </div>
          ))}
        </div>
      </section>

      <footer>Учебная однопроцессная модель. Хеширование обеспечивает контроль целостности, подписи — аутентичность; конфиденциальность не заявляется.</footer>
    </main>
  );
}

export default App;

