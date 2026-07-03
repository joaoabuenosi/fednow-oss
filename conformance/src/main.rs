//! fednow-conformance CLI.
//!
//! ```sh
//! fednow-conformance vectors [dir]        # run the vector corpus (self-check)
//! fednow-conformance validate <paths...>  # judge message files / directories
//! fednow-conformance scenarios <base-url> # drive the CTP scenarios live
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fednow_conformance::check::{check, Direction};
use fednow_conformance::{scenarios, vectors};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("vectors") => cmd_vectors(args.get(1).map(PathBuf::from)),
        Some("validate") if args.len() > 1 => cmd_validate(&args[1..]),
        Some("scenarios") if args.len() == 2 => cmd_scenarios(&args[1]),
        _ => {
            eprintln!(
                "usage:\n  fednow-conformance vectors [dir]\n  fednow-conformance validate <paths...>\n  fednow-conformance scenarios <base-url>"
            );
            ExitCode::from(2)
        }
    }
}

fn cmd_vectors(dir: Option<PathBuf>) -> ExitCode {
    let dir = dir.unwrap_or_else(|| PathBuf::from("conformance/vectors"));
    match vectors::run(&dir) {
        Ok(results) => {
            let mut failed = 0;
            for r in &results {
                if r.passed {
                    println!("PASS {}", r.file);
                } else {
                    failed += 1;
                    println!("FAIL {} — {}", r.file, r.detail);
                }
            }
            println!("{} vector(s), {} failed", results.len(), failed);
            if failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_validate(paths: &[String]) -> ExitCode {
    let mut files = Vec::new();
    for p in paths {
        collect(Path::new(p), &mut files);
    }
    if files.is_empty() {
        eprintln!("no XML files found");
        return ExitCode::from(2);
    }
    let mut failed = 0;
    for file in &files {
        match std::fs::read_to_string(file) {
            Ok(xml) => match check(&xml, Direction::Either) {
                Ok(v) if v.issues.is_empty() => {
                    println!("OK   [{:9}] {}", v.message_type, file.display())
                }
                Ok(v) => {
                    failed += 1;
                    println!(
                        "FAIL [{:9}] {} — {}",
                        v.message_type,
                        file.display(),
                        v.issues
                            .iter()
                            .map(|i| i.code)
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
                Err(e) => {
                    failed += 1;
                    println!("FAIL {} — {e}", file.display());
                }
            },
            Err(e) => {
                failed += 1;
                println!("FAIL {} — cannot read: {e}", file.display());
            }
        }
    }
    println!("{} file(s), {} failed", files.len(), failed);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut children: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            children.sort();
            for child in children {
                collect(&child, out);
            }
        }
    } else if path.extension().is_some_and(|e| e == "xml") {
        out.push(path.to_path_buf());
    }
}

fn cmd_scenarios(base_url: &str) -> ExitCode {
    let results = scenarios::run(base_url);
    let mut failed = 0;
    for r in &results {
        if r.passed {
            println!("PASS {}", r.name);
        } else {
            failed += 1;
            println!("FAIL {} — {}", r.name, r.detail);
        }
    }
    println!("{} scenario(s), {} failed", results.len(), failed);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
