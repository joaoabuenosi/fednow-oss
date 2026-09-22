//! The binary fails closed: without credentials it refuses to start.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Run the real binary with a scrubbed environment plus `env`, and return its
/// exit status and stderr. Kills it and fails if it is still running after a
/// few seconds — a gateway that starts is exactly the bug being tested for.
fn run_gateway(env: &[(&str, &str)]) -> (std::process::ExitStatus, String) {
    let db = std::env::temp_dir().join(format!("fednow-gw-startup-{}.db", std::process::id()));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fednow-gateway"));
    cmd.env_clear()
        // Port 0 and a throwaway DB, so a regression cannot collide with
        // anything even while it wrongly runs.
        .env("FEDNOW_GW_ADDR", "127.0.0.1:0")
        .env("FEDNOW_GW_DB", &db)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn fednow-gateway");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait on fednow-gateway") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            std::fs::remove_file(&db).ok();
            panic!("fednow-gateway started without valid credentials: {env:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = child.wait_with_output().expect("collect stderr");
    std::fs::remove_file(&db).ok();
    (status, String::from_utf8_lossy(&output.stderr).into_owned())
}

#[test]
fn refuses_to_start_without_api_keys() {
    for env in [
        &[][..],
        &[("FEDNOW_GW_API_KEYS", "")][..],
        &[("FEDNOW_GW_API_KEYS", " , ")][..],
        // Read-only keys alone do not count: nobody could submit, and the
        // variable that matters is still missing.
        &[(
            "FEDNOW_GW_READ_API_KEYS",
            "test-only-read-only-key-0123456789abcdef",
        )][..],
    ] {
        let (status, stderr) = run_gateway(env);
        assert!(!status.success(), "{env:?}");
        assert!(stderr.contains("FEDNOW_GW_API_KEYS"), "{stderr}");
        assert!(stderr.contains("refusing to start"), "{stderr}");
        assert!(!stderr.contains("listening"), "{stderr}");
    }
}

#[test]
fn refuses_a_weak_key_without_printing_it() {
    let (status, stderr) = run_gateway(&[("FEDNOW_GW_API_KEYS", "test-only-too-short")]);
    assert!(!status.success());
    assert!(stderr.contains("shorter than 32"), "{stderr}");
    assert!(!stderr.contains("test-only-too-short"), "{stderr}");
    assert!(!stderr.contains("listening"), "{stderr}");
}

#[test]
fn a_normal_start_never_writes_a_key_to_stderr() {
    use std::io::{BufRead, BufReader};

    const FULL: &str = "test-only-full-access-key-0123456789abcdef";
    const READ: &str = "test-only-read-only-key-0123456789abcdef";
    let db = std::env::temp_dir().join(format!("fednow-gw-start-{}.db", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_fednow-gateway"))
        .env_clear()
        .env("FEDNOW_GW_ADDR", "127.0.0.1:0")
        .env("FEDNOW_GW_DB", &db)
        .env("FEDNOW_GW_API_KEYS", FULL)
        .env("FEDNOW_GW_READ_API_KEYS", READ)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fednow-gateway");

    // Collect stderr until the gateway says it is listening (or 10s pass).
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let listening = line.contains("listening");
                seen.push(line);
                if listening {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    child.kill().ok();
    let stdout = child.wait_with_output().expect("collect stdout").stdout;
    std::fs::remove_file(&db).ok();

    let stderr = seen.join("\n");
    assert!(
        stderr.contains("listening"),
        "gateway did not start: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&stdout);
    for output in [&stderr[..], &stdout[..]] {
        assert!(
            !output.contains("test-only"),
            "a key reached the output: {output}"
        );
    }
}
