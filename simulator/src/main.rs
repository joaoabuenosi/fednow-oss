//! fednow-sim entry point: HTTP dev mode.

use fednow_sim::{router, SimConfig};

#[tokio::main]
async fn main() {
    let config = load_config();
    let addr = std::env::var("FEDNOW_SIM_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind {addr}: {e}"));
    eprintln!("fednow-sim listening on {addr} (POST pacs.008 to /fednow/messages)");

    axum::serve(listener, router(config))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server error");
}

fn load_config() -> SimConfig {
    let path = std::env::var("FEDNOW_SIM_CONFIG").unwrap_or_else(|_| "fednow-sim.toml".to_string());
    match std::fs::read_to_string(&path) {
        Ok(text) => match SimConfig::from_toml(&text) {
            Ok(c) => {
                eprintln!("loaded {} scenario(s) from {path}", c.scenarios.len());
                c
            }
            Err(e) => panic!("invalid config {path}: {e}"),
        },
        Err(_) => SimConfig::default(),
    }
}
