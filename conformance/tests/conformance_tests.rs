//! Self-conformance: fednow-core must pass its own corpus, and fednow-sim
//! must pass the live CTP scenario runner.

use std::path::Path;

use fednow_conformance::{scenarios, vectors};

#[test]
fn the_vector_corpus_passes_against_fednow_core() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors");
    let results = vectors::run(&dir).expect("corpus must load");
    assert!(!results.is_empty());
    let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    assert!(failures.is_empty(), "corpus failures: {failures:#?}");
}

#[test]
fn fednow_sim_passes_the_ctp_scenarios() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, fednow_sim::router(Default::default()))
                .await
                .unwrap();
        });
    });
    let base_url = format!("http://{}", rx.recv().unwrap());

    let results = scenarios::run(&base_url);
    let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    assert!(failures.is_empty(), "scenario failures: {failures:#?}");
}
