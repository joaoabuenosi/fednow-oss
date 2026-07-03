//! fednow-gateway entry point: REST northbound + background reconciler,
//! southbound pointed at fednow-sim (development wiring).

use std::sync::Arc;

use chrono::Utc;
use fednow_gateway::http::{router, AppState, ReconcileConfig};
use fednow_gateway::{AnyPort, HttpSimPort, MqSimPort, PaymentService, SqliteStore};

#[tokio::main]
async fn main() {
    let addr = std::env::var("FEDNOW_GW_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".to_string());
    let sim_url =
        std::env::var("FEDNOW_GW_SIM_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let sender_rtn =
        std::env::var("FEDNOW_GW_SENDER_RTN").unwrap_or_else(|_| "021040078".to_string());
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

    let store =
        SqliteStore::open(&db_path).unwrap_or_else(|e| panic!("cannot open {db_path}: {e}"));
    eprintln!("event store: {db_path}");
    let state = Arc::new(AppState {
        service: PaymentService::new(store, port, sender_rtn),
        reconcile,
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
