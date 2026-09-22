//! Northbound authentication and authorization: static bearer API keys.
//!
//! Clients send `Authorization: Bearer <key>`. Keys come in three tiers:
//!
//! | Tier | Environment | May call |
//! |---|---|---|
//! | full | `FEDNOW_GW_API_KEYS` (required) | [`Access::Read`] and [`Access::Write`] routes |
//! | read-only | `FEDNOW_GW_READ_API_KEYS` (optional) | [`Access::Read`] routes only |
//! | operator | `FEDNOW_GW_OPERATOR_API_KEYS` (optional) | [`Access::Read`] and [`Access::Operate`] routes |
//!
//! All three variables are comma-separated lists, so a key can be rotated
//! without downtime: add the new key, move clients over, drop the old one.
//!
//! **Separation of duties.** [`Access::Operate`] routes release or cancel a
//! payment the pre-send risk check held. A full-access key cannot call them:
//! otherwise the application that submits payments could release its own
//! holds, and the check would stop nothing. An operator key cannot submit.
//! With no operator key configured, nobody can release a hold.
//!
//! **Attribution.** Each operator key is configured as `label:key`. The label
//! (a short code, e.g. `ops-alice`) is what the event history records as the
//! actor, `operator:<label>`: a name the gateway derives from the credential,
//! which a request cannot choose. Map labels to people in your own directory;
//! the gateway stores no personal data.
//!
//! **Fail closed.** There is no way to build an [`ApiKeys`] with zero
//! full-access keys, and the router cannot be built without an [`ApiKeys`]
//! (it lives in [`crate::http::AppState`]), so a gateway that was given no
//! credentials refuses to start instead of serving unauthenticated.
//!
//! **Hygiene.** Only SHA-256 digests of the configured keys are kept. A
//! presented key is hashed and compared against *every* configured digest in
//! constant time, with no early exit, so neither the comparison nor the
//! position of a match leaks through timing. No key, configured or presented,
//! ever appears in an error message, a response body or a log line;
//! [`AuthConfigError`] names the variable and the entry's position instead.

use std::fmt;
use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ring::digest::{digest, SHA256};
use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

/// Environment variable holding the full-access keys (required).
pub const API_KEYS_ENV: &str = "FEDNOW_GW_API_KEYS";
/// Environment variable holding the read-only keys (optional).
pub const READ_API_KEYS_ENV: &str = "FEDNOW_GW_READ_API_KEYS";
/// Environment variable holding the operator keys, as `label:key` (optional).
pub const OPERATOR_API_KEYS_ENV: &str = "FEDNOW_GW_OPERATOR_API_KEYS";
/// Shortest key accepted. `openssl rand -hex 32` produces 64 characters.
pub const MIN_KEY_LEN: usize = 32;

const REALM: &str = "fednow-gateway";

/// What a route requires of the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// No credential needed. Reserved for probes (`/healthz`).
    Public,
    /// Any valid key, full-access or read-only.
    Read,
    /// A full-access key.
    Write,
    /// An operator key: resolve payments the risk check held.
    Operate,
}

/// The tier a presented key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Full,
    ReadOnly,
    Operator,
}

impl Role {
    /// Whether this tier may call a route requiring `access`.
    pub fn may(self, access: Access) -> bool {
        match access {
            Access::Public | Access::Read => true,
            Access::Write => self == Role::Full,
            Access::Operate => self == Role::Operator,
        }
    }
}

/// Who is calling, as the middleware established it. Handlers receive it as a
/// request extension; it never carries the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub role: Role,
    /// The operator key's label; `None` for the other tiers.
    pub label: Option<String>,
}

impl Caller {
    /// The actor an audit event records for this caller.
    pub fn actor(&self) -> String {
        match (&self.role, &self.label) {
            (Role::Operator, Some(label)) => format!("operator:{label}"),
            (Role::Operator, None) => "operator".to_string(),
            (Role::Full, _) => "full".to_string(),
            (Role::ReadOnly, _) => "read_only".to_string(),
        }
    }
}

/// Why the key configuration was refused. Never carries a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthConfigError {
    /// No full-access key configured: the gateway would run unauthenticated.
    NoFullAccessKeys,
    /// Entry `position` (1-based) of `var` is shorter than [`MIN_KEY_LEN`].
    TooShort { var: &'static str, position: usize },
    /// Entry `position` (1-based) of `var` has a character outside visible
    /// ASCII (`!`..=`~`), so it cannot travel in a header unambiguously.
    InvalidCharacter { var: &'static str, position: usize },
    /// The same key is configured in two tiers.
    KeyInBothTiers,
    /// Entry `position` (1-based) of the operator list is not `label:key`
    /// with a label that is a short code (`[a-z][a-z0-9_.-]{0,31}`, no run of
    /// five digits).
    InvalidOperatorLabel { position: usize },
    /// Entry `position` (1-based) of the operator list reuses a label.
    DuplicateOperatorLabel { position: usize },
}

impl fmt::Display for AuthConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFullAccessKeys => write!(
                f,
                "{API_KEYS_ENV} is not set or holds no key; refusing to start without \
                 authentication. Set it to one or more comma-separated keys of at least \
                 {MIN_KEY_LEN} characters (e.g. the output of `openssl rand -hex 32`)"
            ),
            Self::TooShort { var, position } => write!(
                f,
                "{var}: key #{position} is shorter than {MIN_KEY_LEN} characters"
            ),
            Self::InvalidCharacter { var, position } => write!(
                f,
                "{var}: key #{position} contains a character outside visible ASCII \
                 (whitespace and control characters are not allowed)"
            ),
            Self::KeyInBothTiers => write!(
                f,
                "a key appears in more than one of {API_KEYS_ENV}, {READ_API_KEYS_ENV} and \
                 {OPERATOR_API_KEYS_ENV}; a key must belong to exactly one tier"
            ),
            Self::InvalidOperatorLabel { position } => write!(
                f,
                "{OPERATOR_API_KEYS_ENV}: entry #{position} must be label:key, with a label of \
                 1-32 characters [a-z0-9_.-] starting with a letter (no run of five digits)"
            ),
            Self::DuplicateOperatorLabel { position } => write!(
                f,
                "{OPERATOR_API_KEYS_ENV}: entry #{position} reuses a label; each operator key \
                 needs its own label"
            ),
        }
    }
}

impl std::error::Error for AuthConfigError {}

type KeyDigest = [u8; 32];

/// The configured keys, as digests. Cheap to clone.
#[derive(Clone)]
pub struct ApiKeys {
    inner: Arc<Digests>,
}

struct Digests {
    full: Vec<KeyDigest>,
    read_only: Vec<KeyDigest>,
    /// (digest, label)
    operators: Vec<(KeyDigest, String)>,
}

impl fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only — not even the digests.
        f.debug_struct("ApiKeys")
            .field("full", &self.inner.full.len())
            .field("read_only", &self.inner.read_only.len())
            .field("operators", &self.inner.operators.len())
            .finish()
    }
}

impl ApiKeys {
    /// Read [`API_KEYS_ENV`], [`READ_API_KEYS_ENV`] and
    /// [`OPERATOR_API_KEYS_ENV`].
    pub fn from_env() -> Result<Self, AuthConfigError> {
        Self::from_lists(
            std::env::var(API_KEYS_ENV).ok().as_deref(),
            std::env::var(READ_API_KEYS_ENV).ok().as_deref(),
        )?
        .with_operators(std::env::var(OPERATOR_API_KEYS_ENV).ok().as_deref())
    }

    /// Add operator keys: a comma-separated list of `label:key` entries (the
    /// label ends at the first `:`). `None` or an empty list adds none.
    pub fn with_operators(self, operators: Option<&str>) -> Result<Self, AuthConfigError> {
        let mut parsed: Vec<(KeyDigest, String)> = Vec::new();
        for (i, entry) in operators
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .enumerate()
        {
            let position = i + 1;
            let Some((label, key)) = entry.split_once(':') else {
                return Err(AuthConfigError::InvalidOperatorLabel { position });
            };
            if label.len() > 32 || crate::risk::sanitize_reason(label) != label {
                return Err(AuthConfigError::InvalidOperatorLabel { position });
            }
            if parsed.iter().any(|(_, l)| l == label) {
                return Err(AuthConfigError::DuplicateOperatorLabel { position });
            }
            let digest = parse_key(OPERATOR_API_KEYS_ENV, position, key)?;
            parsed.push((digest, label.to_string()));
        }
        let inner = &self.inner;
        let in_other_tier = |d: &KeyDigest| inner.full.contains(d) || inner.read_only.contains(d);
        if parsed.iter().enumerate().any(|(i, (d, _))| {
            in_other_tier(d) || parsed[..i].iter().any(|(earlier, _)| earlier == d)
        }) {
            return Err(AuthConfigError::KeyInBothTiers);
        }
        Ok(Self {
            inner: Arc::new(Digests {
                full: inner.full.clone(),
                read_only: inner.read_only.clone(),
                operators: parsed,
            }),
        })
    }

    /// Parse two comma-separated lists. Surrounding whitespace and empty
    /// entries (a trailing comma) are ignored; `full` must yield at least one
    /// key.
    pub fn from_lists(
        full: Option<&str>,
        read_only: Option<&str>,
    ) -> Result<Self, AuthConfigError> {
        let full = parse_list(API_KEYS_ENV, full.unwrap_or(""))?;
        let read_only = parse_list(READ_API_KEYS_ENV, read_only.unwrap_or(""))?;
        if full.is_empty() {
            return Err(AuthConfigError::NoFullAccessKeys);
        }
        if full.iter().any(|f| read_only.contains(f)) {
            return Err(AuthConfigError::KeyInBothTiers);
        }
        Ok(Self {
            inner: Arc::new(Digests {
                full,
                read_only,
                operators: Vec::new(),
            }),
        })
    }

    /// Number of (full-access, read-only) keys configured.
    pub fn counts(&self) -> (usize, usize) {
        (self.inner.full.len(), self.inner.read_only.len())
    }

    /// Number of operator keys configured.
    pub fn operator_count(&self) -> usize {
        self.inner.operators.len()
    }

    /// The tier `presented` belongs to, or `None` if it matches no key.
    /// Constant-time in the key's content: every digest is compared.
    pub fn role(&self, presented: &[u8]) -> Option<Role> {
        self.identify(presented).map(|c| c.role)
    }

    /// Who `presented` identifies, or `None` if it matches no key. Every
    /// digest of every tier is compared, with no early exit, and the matching
    /// operator's position is selected in constant time too.
    pub fn identify(&self, presented: &[u8]) -> Option<Caller> {
        let d = sha256(presented);
        let any = |set: &[KeyDigest]| set.iter().fold(Choice::from(0), |acc, k| acc | k.ct_eq(&d));
        let full = any(&self.inner.full);
        let read_only = any(&self.inner.read_only);
        let mut operator = Choice::from(0);
        let mut index = 0u64;
        for (i, (k, _)) in self.inner.operators.iter().enumerate() {
            let hit = k.ct_eq(&d);
            operator |= hit;
            index.conditional_assign(&(i as u64), hit);
        }
        if bool::from(full) {
            Some(Caller {
                role: Role::Full,
                label: None,
            })
        } else if bool::from(read_only) {
            Some(Caller {
                role: Role::ReadOnly,
                label: None,
            })
        } else if bool::from(operator) {
            let label = usize::try_from(index)
                .ok()
                .and_then(|i| self.inner.operators.get(i))
                .map(|(_, label)| label.clone());
            Some(Caller {
                role: Role::Operator,
                label,
            })
        } else {
            None
        }
    }
}

fn parse_list(var: &'static str, raw: &str) -> Result<Vec<KeyDigest>, AuthConfigError> {
    let mut out = Vec::new();
    for (i, key) in raw
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .enumerate()
    {
        out.push(parse_key(var, i + 1, key)?);
    }
    Ok(out)
}

fn parse_key(var: &'static str, position: usize, key: &str) -> Result<KeyDigest, AuthConfigError> {
    if !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AuthConfigError::InvalidCharacter { var, position });
    }
    if key.len() < MIN_KEY_LEN {
        return Err(AuthConfigError::TooShort { var, position });
    }
    Ok(sha256(key.as_bytes()))
}

fn sha256(bytes: &[u8]) -> KeyDigest {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest(&SHA256, bytes).as_ref());
    out
}

/// One registered route and what it requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSpec {
    pub method: Method,
    pub path: &'static str,
    pub access: Access,
}

/// Middleware state: the keys plus the access table of every route.
#[derive(Clone)]
pub(crate) struct Guard {
    pub(crate) keys: ApiKeys,
    pub(crate) routes: Arc<Vec<RouteSpec>>,
}

impl Guard {
    /// Access required for `method` on the matched route template. A route
    /// missing from the table — one added without stating its access —
    /// requires a full-access key: protected by default.
    fn required(&self, method: &Method, matched: Option<&str>) -> Access {
        // axum answers HEAD with the GET handler; judge it the same way.
        let method = if method == Method::HEAD {
            &Method::GET
        } else {
            method
        };
        matched
            .and_then(|path| {
                self.routes
                    .iter()
                    .find(|r| r.path == path && r.method == *method)
            })
            .map_or(Access::Write, |r| r.access)
    }
}

/// Authenticate (401) then authorize (403) every routed request.
pub(crate) async fn authorize(State(guard): State<Guard>, req: Request, next: Next) -> Response {
    let matched = req
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str);
    let access = guard.required(req.method(), matched);
    if access == Access::Public {
        return next.run(req).await;
    }
    let Some(token) = bearer_token(req.headers()) else {
        return unauthorized(None);
    };
    match guard.keys.identify(token.as_bytes()) {
        None => unauthorized(Some("invalid_token")),
        Some(caller) if !caller.role.may(access) => forbidden(caller.role, access),
        Some(caller) => {
            let mut req = req;
            req.extensions_mut().insert(caller);
            next.run(req).await
        }
    }
}

/// The bearer token, if the request carries exactly one well-formed
/// `Authorization: Bearer <token>` header.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None; // ambiguous: more than one Authorization header
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim_start_matches(' ');
    (!token.is_empty()).then_some(token)
}

fn challenge(error: Option<&str>) -> HeaderValue {
    let value = match error {
        None => format!("Bearer realm=\"{REALM}\""),
        Some(e) => format!("Bearer realm=\"{REALM}\", error=\"{e}\""),
    };
    // Built only from the constants above: always a valid header value.
    HeaderValue::from_str(&value).unwrap_or(HeaderValue::from_static("Bearer"))
}

fn unauthorized(error: Option<&str>) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, challenge(error))],
        Json(serde_json::json!({
            "error": "unauthorized",
            "detail": "a valid API key is required: Authorization: Bearer <key>",
        })),
    )
        .into_response()
}

fn forbidden(role: Role, access: Access) -> Response {
    let detail = match (role, access) {
        (_, Access::Operate) => {
            "this route needs an operator key (FEDNOW_GW_OPERATOR_API_KEYS); \
             full-access and read-only keys cannot resolve a held payment"
        }
        (Role::Operator, _) => "operator keys can read and resolve held payments only",
        _ => "this API key is read-only",
    };
    (
        StatusCode::FORBIDDEN,
        [(
            header::WWW_AUTHENTICATE,
            challenge(Some("insufficient_scope")),
        )],
        Json(serde_json::json!({
            "error": "forbidden",
            "detail": detail,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "test-full-key-0000000000000000000000000000";
    const READ: &str = "test-read-key-0000000000000000000000000000";

    #[test]
    fn no_keys_at_all_is_refused() {
        for (full, read) in [(None, None), (Some(""), None), (Some(" , ,"), Some(READ))] {
            assert_eq!(
                ApiKeys::from_lists(full, read).unwrap_err(),
                AuthConfigError::NoFullAccessKeys
            );
        }
    }

    #[test]
    fn weak_or_malformed_keys_are_refused_without_echoing_them() {
        let err = ApiKeys::from_lists(Some(&format!("{FULL},tooweak")), None).unwrap_err();
        assert_eq!(
            err,
            AuthConfigError::TooShort {
                var: API_KEYS_ENV,
                position: 2
            }
        );
        assert!(!err.to_string().contains("tooweak"), "{err}");

        let spaced = "a key with spaces inside it and long enough to pass";
        let err = ApiKeys::from_lists(Some(spaced), None).unwrap_err();
        assert!(matches!(err, AuthConfigError::InvalidCharacter { .. }));
        assert!(!err.to_string().contains("with spaces"), "{err}");
    }

    #[test]
    fn a_key_in_both_tiers_is_refused() {
        assert_eq!(
            ApiKeys::from_lists(Some(FULL), Some(FULL)).unwrap_err(),
            AuthConfigError::KeyInBothTiers
        );
    }

    #[test]
    fn roles_resolve_and_whitespace_around_entries_is_ignored() {
        let keys = ApiKeys::from_lists(Some(&format!(" {FULL} ,")), Some(READ)).unwrap();
        assert_eq!(keys.counts(), (1, 1));
        assert_eq!(keys.role(FULL.as_bytes()), Some(Role::Full));
        assert_eq!(keys.role(READ.as_bytes()), Some(Role::ReadOnly));
        assert_eq!(
            keys.role(b"test-full-key-000000000000000000000000000X"),
            None
        );
        assert_eq!(keys.role(b""), None);
    }

    #[test]
    fn debug_output_carries_counts_only() {
        let keys = ApiKeys::from_lists(Some(FULL), Some(READ))
            .unwrap()
            .with_operators(Some(&format!("ops-a:{OPS}")))
            .unwrap();
        let shown = format!("{keys:?}");
        assert_eq!(shown, "ApiKeys { full: 1, read_only: 1, operators: 1 }");
    }

    const OPS: &str = "test-ops1-key-0000000000000000000000000000";
    const OPS2: &str = "test-ops2-key-0000000000000000000000000000";

    #[test]
    fn operator_keys_carry_their_label_and_only_their_tier() {
        let keys = ApiKeys::from_lists(Some(FULL), Some(READ))
            .unwrap()
            .with_operators(Some(&format!(" ops-a:{OPS} , ops.b:{OPS2},")))
            .unwrap();
        assert_eq!(keys.operator_count(), 2);
        let b = keys.identify(OPS2.as_bytes()).unwrap();
        assert_eq!(b.role, Role::Operator);
        assert_eq!(b.actor(), "operator:ops.b");
        assert_eq!(
            keys.identify(OPS.as_bytes()).unwrap().actor(),
            "operator:ops-a"
        );
        assert_eq!(keys.identify(FULL.as_bytes()).unwrap().label, None);

        // The access matrix: nobody but an operator resolves a hold, and an
        // operator cannot submit.
        for (role, read, write, operate) in [
            (Role::Full, true, true, false),
            (Role::ReadOnly, true, false, false),
            (Role::Operator, true, false, true),
        ] {
            assert_eq!(role.may(Access::Read), read, "{role:?}");
            assert_eq!(role.may(Access::Write), write, "{role:?}");
            assert_eq!(role.may(Access::Operate), operate, "{role:?}");
        }
    }

    #[test]
    fn operator_entries_are_validated_without_echoing_keys() {
        let base = || ApiKeys::from_lists(Some(FULL), None).unwrap();
        for (entry, want) in [
            (
                OPS.to_string(),
                AuthConfigError::InvalidOperatorLabel { position: 1 },
            ),
            (
                format!("Ops:{OPS}"),
                AuthConfigError::InvalidOperatorLabel { position: 1 },
            ),
            (
                format!("emp1234567:{OPS}"),
                AuthConfigError::InvalidOperatorLabel { position: 1 },
            ),
            (
                format!(":{OPS}"),
                AuthConfigError::InvalidOperatorLabel { position: 1 },
            ),
            (
                format!("a:{OPS},a:{OPS2}"),
                AuthConfigError::DuplicateOperatorLabel { position: 2 },
            ),
            (format!("a:{FULL}"), AuthConfigError::KeyInBothTiers),
            (format!("a:{OPS},b:{OPS}"), AuthConfigError::KeyInBothTiers),
            (
                "a:weakkey".to_string(),
                AuthConfigError::TooShort {
                    var: OPERATOR_API_KEYS_ENV,
                    position: 1,
                },
            ),
        ] {
            let err = base().with_operators(Some(&entry)).unwrap_err();
            assert_eq!(err, want, "{entry}");
            let shown = err.to_string();
            assert!(!shown.contains("test-"), "{shown}");
            assert!(!shown.contains("weakkey"), "{shown}");
        }
        assert_eq!(base().with_operators(None).unwrap().operator_count(), 0);
    }

    #[test]
    fn bearer_parsing_is_strict() {
        let parse = |values: &[&str]| {
            let mut h = HeaderMap::new();
            for v in values {
                h.append(header::AUTHORIZATION, HeaderValue::from_str(v).unwrap());
            }
            bearer_token(&h).map(str::to_string)
        };
        assert_eq!(parse(&["Bearer abc"]), Some("abc".into()));
        assert_eq!(parse(&["bearer abc"]), Some("abc".into()));
        assert_eq!(parse(&["Basic abc"]), None);
        assert_eq!(parse(&["Bearer "]), None);
        assert_eq!(parse(&["Bearer"]), None);
        assert_eq!(parse(&["Bearer abc", "Bearer abc"]), None);
        assert_eq!(parse(&[]), None);
    }
}
