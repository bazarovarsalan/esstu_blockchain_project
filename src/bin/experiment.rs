use std::{
    hint::black_box,
    process::Command,
    time::{Duration, Instant},
};

use round_robin_quorum::Network;

const BATCH_SIZES: [usize; 4] = [1, 10, 100, 500];
const ACTIVE_COUNTS: [usize; 3] = [4, 3, 2];
const WARMUPS: usize = 2;
const SAMPLES: usize = 7;

#[derive(Debug)]
struct Measurement {
    batch_size: usize,
    active_validators: usize,
    formation: Duration,
    verification: Duration,
    consensus: Duration,
    votes: usize,
    confirmed: bool,
}

fn command_output(program: &str, arguments: &[&str]) -> String {
    Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "не определено".to_string())
}

fn prepared_network(batch_size: usize, active_validators: usize) -> Network {
    let mut network = Network::new();
    for index in active_validators..4 {
        network
            .set_validator_active(&format!("validator-{}", index + 1), false)
            .expect("validator exists");
    }
    network
        .create_and_submit_batch("Баир", "Бато", 1, batch_size)
        .expect("benchmark transaction batch is valid");
    network
}

fn one_measurement(batch_size: usize, active_validators: usize) -> Measurement {
    let mut network = prepared_network(batch_size, active_validators);

    let formation_started = Instant::now();
    let candidate = black_box(network.build_candidate().expect("candidate is formed"));
    let formation = formation_started.elapsed();

    let verification_started = Instant::now();
    let validation_results = black_box(network.validate_candidate_on_active_nodes(&candidate));
    let verification = verification_started.elapsed();
    assert!(validation_results.iter().all(Result::is_ok));
    assert_eq!(validation_results.len(), active_validators);

    let consensus_started = Instant::now();
    let attempt = black_box(
        network
            .produce_block()
            .expect("consensus attempt completes"),
    );
    let consensus = consensus_started.elapsed();

    Measurement {
        batch_size,
        active_validators,
        formation,
        verification,
        consensus,
        votes: attempt.votes.len(),
        confirmed: attempt.confirmed,
    }
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn aggregate(batch_size: usize, active_validators: usize) -> Measurement {
    for _ in 0..WARMUPS {
        black_box(one_measurement(batch_size, active_validators));
    }
    let measurements = (0..SAMPLES)
        .map(|_| one_measurement(batch_size, active_validators))
        .collect::<Vec<_>>();
    Measurement {
        batch_size,
        active_validators,
        formation: median(measurements.iter().map(|item| item.formation).collect()),
        verification: median(measurements.iter().map(|item| item.verification).collect()),
        consensus: median(measurements.iter().map(|item| item.consensus).collect()),
        votes: measurements[0].votes,
        confirmed: measurements[0].confirmed,
    }
}

fn micros(duration: Duration) -> u128 {
    duration.as_micros()
}

fn main() {
    println!("# Результаты воспроизводимого эксперимента RoundRobinQuorum\n");
    println!(
        "- Дата запуска (UTC): {}",
        command_output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
    );
    println!("- ОС: {}", command_output("uname", &["-srm"]));
    println!("- Rust: {}", command_output("rustc", &["--version"]));
    println!("- Режим: Cargo `--release`");
    println!("- Прогревочных запусков: {WARMUPS}; измеряемых запусков: {SAMPLES}");
    println!(
        "- Агрегирование: медиана; подготовка и подписание входного пакета в измеряемый участок не входят.\n"
    );
    println!(
        "| Транзакций | Активных валидаторов | Формирование, мкс | Независимая проверка всеми активными, мкс | Полная попытка консенсуса, мкс | Голосов | Подтверждён |"
    );
    println!("|---:|---:|---:|---:|---:|---:|:---:|");

    for batch_size in BATCH_SIZES {
        for active_validators in ACTIVE_COUNTS {
            let result = aggregate(batch_size, active_validators);
            println!(
                "| {} | {} | {} | {} | {} | {} | {} |",
                result.batch_size,
                result.active_validators,
                micros(result.formation),
                micros(result.verification),
                micros(result.consensus),
                result.votes,
                if result.confirmed { "да" } else { "нет" },
            );
        }
    }

    println!(
        "\nИзмерения характеризуют только данный однопроцессный учебный прототип и конкретные условия запуска. Они не включают реальный сетевой транспорт и не могут использоваться для выводов о производительности промышленных блокчейн-платформ."
    );
}
