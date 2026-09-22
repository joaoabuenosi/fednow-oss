//! fednow-gateway entry point: REST northbound + background reconciler,
//! southbound pointed at fednow-sim (development wiring).

use std::sync::Arc;

use chrono::Utc;
use fednow_gateway::hold::hold_policy_from_lookup;
use fednow_gateway::http::{router, AppState, ReconcileConfig};
use fednow_gateway::risk::gate_from_lookup;
use fednow_gateway::{AnyPort, ApiKeys, HttpSimPort, MqSimPort, PaymentService, SqliteStore};

#[tokio::main]
async fn main() {
    // Credentials first: a gateway without them must not open its store,
    // start its sweeper or bind its port. Fail closed, never run open.
    let api_keys = match ApiKeys::from_env() {
        Ok(keys) => keys,
        Err(e) => {
            eprintln!("fednow-gateway: {e}");
            std::process::exit(2);
        }
    };
    // A fixed line on purpose: nothing derived from the keys (not even how
    // many there are) goes to the log, so there is no data flow from the
    // credential store to stderr for anyone, or any analyser, to audit.
    eprintln!("api keys: loaded; authentication required on every route but /healthz");

    let addr = std::env::var("FEDNOW_GW_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".to_string());
    let sim_url =
        std::env::var("FEDNOW_GW_SIM_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let sender_rtn =
        std::env::var("FEDNOW_GW_SENDER_RTN").unwrap_or_else(|_| "991000009".to_string());
    let db_path = std::env::var("FEDNOW_GW_DB").unwrap_or_else(|_| "fednow-gateway.db".to_string());
    let reconcile = ReconcileConfig {
        timeout_secs: env_i64("FEDNOW_GW_TIMEOUT_SECS", 20),
        backoff_secs: env_i64("FEDNOW_GW_BACKOFF_SECS", 30),
    };
    let sweep_secs = env_i64("FEDNOW_GW_SWEEP_SECS", 10).max(1) as u64;

    // Southbound flavor: "http" (synchronous dev mode, default) or "mq"
    // (fire-and-forget sends + advice queue — the production semantics).
    let southbound = std::env::var("FEDNOW_GW_SOUTHBOUND").unwrap_or_else(|_| "http".to_string());
    let port = match southbound.as_str() {
        "mq" => AnyPort::Mq(MqSimPort::new(sim_url.clone(), sender_rtn.clone())),
        "http" => AnyPort::Http(HttpSimPort::new(sim_url.clone())),
        other => panic!("FEDNOW_GW_SOUTHBOUND must be 'http' or 'mq', found '{other}'"),
    };

    // Pre-send risk check: off unless FEDNOW_GW_RISK_PROVIDER names one. A
    // value the gateway does not understand stops it here, like a missing
    // API key: never run with a check the operator did not intend.
    let risk = match gate_from_lookup(|name| std::env::var(name).ok(), &sim_url) {
        Ok(gate) => gate,
        Err(e) => {
            eprintln!("fednow-gateway: {e}");
            std::process::exit(2);
        }
    };
    // How long a held payment stays releasable. Read always, so a bad value
    // is caught even before a provider is turned on.
    let hold = match hold_policy_from_lookup(|name| std::env::var(name).ok()) {
        Ok(policy) => policy,
        Err(e) => {
            eprintln!("fednow-gateway: {e}");
            std::process::exit(2);
        }
    };
    if risk.is_enabled() {
        let policy = risk.policy();
        eprintln!(
            "risk check: on (provider {}, timeout {} ms, max in flight {}, on unavailable: {})",
            risk.provider_name().unwrap_or("none"),
            policy.timeout.as_millis(),
            policy.max_in_flight,
            policy.on_unavailable.name()
        );
        eprintln!(
            "held payments: releasable for {} s by an operator key, then cancelled (hold_expired)",
            hold.max_age_secs
        );
    } else {
        eprintln!("risk check: off (FEDNOW_GW_RISK_PROVIDER unset or none)");
    }

    let store =
        SqliteStore::open(&db_path).unwrap_or_else(|e| panic!("cannot open {db_path}: {e}"));
    eprintln!("event store: {db_path}");
    let state = Arc::new(AppState {
        service: PaymentService::new(store, port, sender_rtn)
            .with_risk_gate(risk)
            .with_hold_policy(hold),
        reconcile,
        api_keys,
    });

    // Background reconciler: sweeps every payment on an interval. Blocking
    // work on its own thread — the domain owns no clocks, so we pass them in.
    let sweeper = Arc::clone(&state);
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(sweep_secs));
        let now = Utc::now();
        // Retry anything a transport failure left in the outbox…
        sweeper.service.publish_pending(now.timestamp());
        // …drain asynchronously delivered advices (MQ mode; no-op over HTTP)…
        sweeper.service.pump_advices(now.timestamp());
        // …cancel holds nobody released in time (no-op without a provider)…
        sweeper.service.expire_holds(now.timestamp());
        // …then run the timeout/query policy over every payment.
        let errors = sweeper.service.reconcile_all(
            &now.format("%Y%m%d").to_string(),
            now.timestamp(),
            sweeper.reconcile.timeout_secs,
            sweeper.reconcile.backoff_secs,
        );
        for (key, e) in errors {
            eprintln!("reconcile {key}: {e}");
        }
    });

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind {addr}: {e}"));
    eprintln!("fednow-gateway listening on {addr} (southbound: {sim_url})");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server error");
}

fn env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
