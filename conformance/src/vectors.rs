//! The vector corpus: messages plus expected verdicts, in
//! `conformance/vectors/` (manifest.toml + XML files).

use std::path::Path;

use serde::Deserialize;

use crate::check::{check, Direction};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(rename = "vector")]
    pub vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
pub struct Vector {
    pub file: String,
    pub valid: bool,
    #[serde(default)]
    pub codes: Vec<String>,
    pub direction: Option<String>,
}

#[derive(Debug)]
pub struct VectorResult {
    pub file: String,
    pub passed: bool,
    pub detail: String,
}

/// Run the corpus at `dir` (containing `manifest.toml`) through fednow-core.
pub fn run(dir: &Path) -> Result<Vec<VectorResult>, String> {
    let manifest_text = std::fs::read_to_string(dir.join("manifest.toml"))
        .map_err(|e| format!("cannot read manifest.toml: {e}"))?;
    let manifest: Manifest =
        toml::from_str(&manifest_text).map_err(|e| format!("invalid manifest: {e}"))?;

    let mut results = Vec::new();
    for vector in &manifest.vectors {
        let path = dir.join(&vector.file);
        let xml = match std::fs::read_to_string(&path) {
            Ok(x) => x,
            Err(e) => {
                results.push(VectorResult {
                    file: vector.file.clone(),
                    passed: false,
                    detail: format!("cannot read: {e}"),
                });
                continue;
            }
        };
        let direction = match vector.direction.as_deref() {
            Some("participant") => Direction::Participant,
            Some("service") => Direction::Service,
            _ => Direction::Either,
        };
        let result = match check(&xml, direction) {
            Ok(verdict) => {
                let is_valid = verdict.issues.is_empty();
                if is_valid != vector.valid {
                    VectorResult {
                        file: vector.file.clone(),
                        passed: false,
                        detail: format!(
                            "expected valid={}, got valid={is_valid} ({:?})",
                            vector.valid,
                            verdict.issues.iter().map(|i| i.code).collect::<Vec<_>>()
                        ),
                    }
                } else {
                    let found: Vec<&str> = verdict.issues.iter().map(|i| i.code).collect();
                    let missing: Vec<&String> = vector
                        .codes
                        .iter()
                        .filter(|c| !found.contains(&c.as_str()))
                        .collect();
                    if missing.is_empty() {
                        VectorResult {
                            file: vector.file.clone(),
                            passed: true,
                            detail: "ok".to_string(),
                        }
                    } else {
                        VectorResult {
                            file: vector.file.clone(),
                            passed: false,
                            detail: format!("expected codes {missing:?} not found in {found:?}"),
                        }
                    }
                }
            }
            Err(e) => VectorResult {
                file: vector.file.clone(),
                passed: false,
                detail: format!("check failed: {e}"),
            },
        };
        results.push(result);
    }
    Ok(results)
}
