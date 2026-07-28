//! Offline JWT (RS256) licensing for `pageseeds-cli`.
//!
//! - Store path: `PAGESEEDS_LICENSE_PATH` or `dirs::config_dir()/pageseeds/license.jwt`
//! - Verify signature + claims locally; no network / phone-home
//! - Production embeds **public** PEM only (`public_key.pem`); private key lives with website minting

use jsonwebtoken::errors::{Error as JwtError, ErrorKind as JwtErrorKind};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Production public key (RS256). Matching private key is held by the website license mint
/// (pageseeds repo / commercial backend) — never shipped in this binary.
const PRODUCTION_PUBLIC_KEY_PEM: &str = include_str!("public_key.pem");

/// Paid CLI tools — must match `docs/CLI_COMMERCIAL.md` (25 tools).
static PAID_TOOLS: &[&str] = &[
    "write-context",
    "write-submit",
    "publish-content",
    "fix-context",
    "fix-submit",
    "merge-context",
    "merge-submit",
    "research-pull",
    "create-articles-from-keywords",
    "create-task",
    "execute-task",
    "cancel-tasks",
    "update-task-status",
    "set-task-status",
    "select-keywords",
    "select-content-review",
    "select-cannibalization",
    "create-tasks-from-approved",
    "set-review-status",
    "create-reddit-replies",
    "run-content-audit",
    "cannibalization-strategy",
    "score-zero-impression-articles",
    "write-feature-spec",
    "compare-rendered",
];

/// Required plan claim value for CLI licenses.
const REQUIRED_PLAN: &str = "cli";

#[cfg(test)]
static TEST_PUBLIC_KEY_OVERRIDE: std::sync::Mutex<Option<&'static str>> =
    std::sync::Mutex::new(None);

/// Claims required (and optional) on a PageSeeds CLI license JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseClaims {
    /// NumericDate — required; verified by jsonwebtoken Validation.
    pub exp: i64,
    /// Must be exactly `"cli"`.
    pub plan: String,
    /// Recommended issued-at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    /// Optional subject (e.g. customer id / email hash).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
}

/// Result of reading and verifying the local license file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LicenseStatus {
    Missing,
    Valid { plan: String, exp: i64 },
    Expired { plan: Option<String>, exp: Option<i64> },
    Invalid { reason: String },
}

/// Resolve license file path: `PAGESEEDS_LICENSE_PATH` or `config_dir/pageseeds/license.jwt`.
pub fn license_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PAGESEEDS_LICENSE_PATH") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let config = dirs::config_dir()
        .ok_or_else(|| "could not resolve config directory for license store".to_string())?;
    Ok(config.join("pageseeds").join("license.jwt"))
}

/// Whether `tool` is in the static paid set (exact name match).
pub fn requires_paid_license(tool: &str) -> bool {
    PAID_TOOLS.contains(&tool)
}

/// Paid tool names (exact CLI subcommand strings). Exposed for inventory invariant tests.
pub fn paid_tools() -> &'static [&'static str] {
    PAID_TOOLS
}

/// Operator-tier CLI tools — dev-machine only, **not** part of the commercial
/// free/paid boundary (docs/CLI_COMMERCIAL.md "Operator tier"). They require
/// external toolchains and a source checkout, need no license, and are never
/// sold to customers.
static OPERATOR_TOOLS: &[&str] = &["video-clip-render"];

/// Whether `tool` is an operator-tier tool (no license, outside free/paid).
pub fn is_operator_tool(tool: &str) -> bool {
    OPERATOR_TOOLS.contains(&tool)
}

/// Operator-tier tool names. Exposed for inventory invariant tests.
pub fn operator_tools() -> &'static [&'static str] {
    OPERATOR_TOOLS
}

/// Activate: verify JWT (signature + claims) then persist raw token string.
pub fn activate(token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("license key is empty".to_string());
    }
    // Verify before write — refuse invalid / expired / wrong plan.
    let _claims = verify_token(token, &public_key_pem()).map_err(VerifyError::into_message)?;
    let path = license_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create license directory: {e}"))?;
    }
    std::fs::write(&path, token).map_err(|e| format!("failed to write license file: {e}"))?;
    Ok(())
}

/// Read local license status (re-verifies file each call).
pub fn status() -> LicenseStatus {
    let path = match license_path() {
        Ok(p) => p,
        Err(e) => return LicenseStatus::Invalid { reason: e },
    };
    if !path.exists() {
        return LicenseStatus::Missing;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return LicenseStatus::Invalid {
                reason: format!("failed to read license file: {e}"),
            }
        }
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return LicenseStatus::Invalid {
            reason: "license file is empty".to_string(),
        };
    }
    let pem = public_key_pem();
    match verify_token(raw, &pem) {
        Ok(claims) => LicenseStatus::Valid {
            plan: claims.plan,
            exp: claims.exp,
        },
        Err(VerifyError::Jwt(ref e)) if matches!(e.kind(), JwtErrorKind::ExpiredSignature) => {
            // Signature path already succeeded; only exp failed. Re-decode without exp
            // validation so status can surface plan/exp instead of lying None fields.
            match decode_claims(raw, &pem, false) {
                Ok(claims) => LicenseStatus::Expired {
                    plan: Some(claims.plan),
                    exp: Some(claims.exp),
                },
                Err(_) => LicenseStatus::Expired {
                    plan: None,
                    exp: None,
                },
            }
        }
        Err(e) => LicenseStatus::Invalid {
            reason: e.into_message(),
        },
    }
}

/// Local-only: delete the license file. No network.
pub fn deactivate() -> Result<(), String> {
    let path = license_path()?;
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("failed to remove license file: {e}"))?;
    }
    Ok(())
}

/// Used by CLI gate for paid tools. Re-reads and re-verifies the store.
pub fn require_valid() -> Result<(), String> {
    match status() {
        LicenseStatus::Valid { .. } => Ok(()),
        LicenseStatus::Missing => Err(
            "no license found. Activate: pageseeds-cli license activate <key> — Buy: https://pageseeds.com"
                .to_string(),
        ),
        LicenseStatus::Expired { .. } => Err(
            "license expired. Activate a new key: pageseeds-cli license activate <key> — Buy: https://pageseeds.com"
                .to_string(),
        ),
        LicenseStatus::Invalid { reason } => Err(format!(
            "license invalid ({reason}). Activate: pageseeds-cli license activate <key> — Buy: https://pageseeds.com"
        )),
    }
}

// ─── Internal ────────────────────────────────────────────────────────────────

/// Structured verify failure — keeps `jsonwebtoken::Error` so callers match
/// `ErrorKind` instead of string-scanning display text.
#[derive(Debug)]
enum VerifyError {
    InvalidKey(String),
    Jwt(JwtError),
    WrongPlan(String),
}

impl VerifyError {
    fn into_message(self) -> String {
        match self {
            VerifyError::InvalidKey(msg) => format!("invalid public key / decoding key: {msg}"),
            VerifyError::Jwt(e) => format!("JWT verification failed: {e}"),
            VerifyError::WrongPlan(got) => {
                format!("invalid plan '{got}' (expected '{REQUIRED_PLAN}')")
            }
        }
    }
}

fn public_key_pem() -> String {
    #[cfg(test)]
    {
        if let Ok(guard) = TEST_PUBLIC_KEY_OVERRIDE.lock() {
            if let Some(pem) = *guard {
                return pem.to_string();
            }
        }
    }
    PRODUCTION_PUBLIC_KEY_PEM.to_string()
}

/// Decode RS256 JWT claims; `validate_exp` controls NumericDate exp check.
fn decode_claims(
    token: &str,
    public_pem: &str,
    validate_exp: bool,
) -> Result<LicenseClaims, VerifyError> {
    let key = DecodingKey::from_rsa_pem(public_pem.as_bytes())
        .map_err(|e| VerifyError::InvalidKey(e.to_string()))?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = validate_exp;
    // We only require exp + plan; sub/iat optional. Disable aud/iss defaults.
    validation.set_required_spec_claims(&["exp"]);
    validation.validate_aud = false;

    let data = decode::<LicenseClaims>(token, &key, &validation).map_err(VerifyError::Jwt)?;
    Ok(data.claims)
}

/// Verify RS256 signature + standard claims (exp) and required plan.
fn verify_token(token: &str, public_pem: &str) -> Result<LicenseClaims, VerifyError> {
    let claims = decode_claims(token, public_pem, true)?;
    if claims.plan != REQUIRED_PLAN {
        return Err(VerifyError::WrongPlan(claims.plan));
    }
    Ok(claims)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::sync::MutexGuard;

    const TEST_PRIVATE_PEM: &str = include_str!("testdata/test_private.pem");
    const TEST_PUBLIC_PEM: &str = include_str!("testdata/test_public.pem");

    /// Holds ENV_LOCK + installs test public key for the duration of a test.
    struct TestLicenseEnv {
        _env: MutexGuard<'static, ()>,
        path: PathBuf,
        prev_path: Option<String>,
    }

    impl TestLicenseEnv {
        fn new() -> Self {
            let env = crate::test_support::ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            {
                let mut o = TEST_PUBLIC_KEY_OVERRIDE.lock().unwrap();
                *o = Some(TEST_PUBLIC_PEM);
            }
            let dir = std::env::temp_dir().join(format!(
                "pageseeds-license-test-{}-{}",
                std::process::id(),
                Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("license.jwt");
            let prev_path = std::env::var("PAGESEEDS_LICENSE_PATH").ok();
            std::env::set_var("PAGESEEDS_LICENSE_PATH", &path);
            // Ensure clean slate
            let _ = std::fs::remove_file(&path);
            Self {
                _env: env,
                path,
                prev_path,
            }
        }
    }

    impl Drop for TestLicenseEnv {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
            match &self.prev_path {
                Some(p) => std::env::set_var("PAGESEEDS_LICENSE_PATH", p),
                None => std::env::remove_var("PAGESEEDS_LICENSE_PATH"),
            }
            if let Ok(mut o) = TEST_PUBLIC_KEY_OVERRIDE.lock() {
                *o = None;
            }
        }
    }

    fn mint(claims: &LicenseClaims) -> String {
        let key = EncodingKey::from_rsa_pem(TEST_PRIVATE_PEM.as_bytes()).expect("test private key");
        encode(&Header::new(Algorithm::RS256), claims, &key).expect("encode")
    }

    fn valid_claims(exp_offset_secs: i64) -> LicenseClaims {
        let now = Utc::now().timestamp();
        LicenseClaims {
            exp: now + exp_offset_secs,
            plan: REQUIRED_PLAN.to_string(),
            iat: Some(now),
            sub: Some("test-user".to_string()),
        }
    }

    #[test]
    fn activate_and_require_valid_ok() {
        let _env = TestLicenseEnv::new();
        let token = mint(&valid_claims(3600));
        activate(&token).expect("activate");
        require_valid().expect("require_valid");
        match status() {
            LicenseStatus::Valid { plan, exp } => {
                assert_eq!(plan, "cli");
                assert!(exp > Utc::now().timestamp());
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_require_valid_fails() {
        let _env = TestLicenseEnv::new();
        assert!(matches!(status(), LicenseStatus::Missing));
        let err = require_valid().unwrap_err();
        assert!(
            err.to_lowercase().contains("license") || err.contains("pageseeds.com"),
            "unexpected err: {err}"
        );
    }

    #[test]
    fn expired_token_fails() {
        let _env = TestLicenseEnv::new();
        let claims = valid_claims(-3600);
        let token = mint(&claims);
        let err = activate(&token).unwrap_err();
        assert!(
            err.to_lowercase().contains("expir") || err.to_lowercase().contains("jwt"),
            "unexpected: {err}"
        );
        // Write expired token manually to exercise status path
        std::fs::write(license_path().unwrap(), &token).unwrap();
        match status() {
            LicenseStatus::Expired {
                plan: Some(plan),
                exp: Some(exp),
            } => {
                assert_eq!(plan, "cli");
                assert_eq!(exp, claims.exp);
            }
            other => panic!("expected Expired with plan/exp, got {other:?}"),
        }
        assert!(require_valid().is_err());
    }

    #[test]
    fn tampered_signature_fails() {
        let _env = TestLicenseEnv::new();
        let token = mint(&valid_claims(3600));
        // Flip last char of signature segment
        let mut parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let mut sig = parts[2].to_string();
        let last = sig.pop().unwrap();
        sig.push(if last == 'A' { 'B' } else { 'A' });
        let sig_ref = sig.as_str();
        parts[2] = sig_ref;
        let bad = parts.join(".");
        let err = activate(&bad).unwrap_err();
        assert!(
            err.to_lowercase().contains("jwt") || err.to_lowercase().contains("verif"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn wrong_plan_fails() {
        let _env = TestLicenseEnv::new();
        let mut claims = valid_claims(3600);
        claims.plan = "desktop".to_string();
        let token = mint(&claims);
        let err = activate(&token).unwrap_err();
        assert!(
            err.to_lowercase().contains("plan"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn deactivate_clears_store() {
        let _env = TestLicenseEnv::new();
        let token = mint(&valid_claims(3600));
        activate(&token).unwrap();
        assert!(matches!(status(), LicenseStatus::Valid { .. }));
        deactivate().unwrap();
        assert!(matches!(status(), LicenseStatus::Missing));
        assert!(require_valid().is_err());
    }

    #[test]
    fn requires_paid_license_true_false() {
        assert!(requires_paid_license("write-context"));
        assert!(requires_paid_license("execute-task"));
        assert!(requires_paid_license("research-pull"));
        assert!(requires_paid_license("compare-rendered"));
        assert!(!requires_paid_license("site-overview"));
        assert!(!requires_paid_license("list-tasks"));
        assert!(!requires_paid_license("gsc-queries"));
        assert!(!requires_paid_license("research-context"));
        assert!(!requires_paid_license("license"));
        assert!(!requires_paid_license("video-clip-render"));
        assert!(!requires_paid_license("unknown-tool-xyz"));
    }

    #[test]
    fn operator_tier_is_disjoint_from_paid() {
        assert!(is_operator_tool("video-clip-render"));
        assert!(!is_operator_tool("write-context"));
        assert!(!is_operator_tool("site-overview"));
        for tool in operator_tools() {
            assert!(
                !requires_paid_license(tool),
                "operator tool '{tool}' must not require a license"
            );
        }
    }

    #[test]
    fn paid_set_has_exact_count() {
        assert_eq!(paid_tools().len(), 25, "must match docs/CLI_COMMERCIAL.md");
        // uniqueness
        let mut v = paid_tools().to_vec();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), 25);
    }

    #[test]
    fn production_public_key_parses() {
        DecodingKey::from_rsa_pem(PRODUCTION_PUBLIC_KEY_PEM.as_bytes())
            .expect("production public_key.pem must be valid RSA PEM");
    }
}
