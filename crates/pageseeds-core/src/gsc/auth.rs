use chrono::Utc;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::gsc::TokenState;

// ─── Service account ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct JwtClaims {
    iss: String,
    scope: String,
    aud: String,
    exp: i64,
    iat: i64,
}

#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
}

pub async fn get_service_account_token(sa_path: &str) -> Result<TokenState> {
    let content = std::fs::read_to_string(sa_path)
        .map_err(|e| Error::Other(format!("Failed to read service account: {}", e)))?;
    let key: ServiceAccountKey = serde_json::from_str(&content)
        .map_err(|e| Error::Other(format!("Invalid service account JSON: {}", e)))?;

    let now = Utc::now().timestamp();
    let claims = JwtClaims {
        iss: key.client_email,
        scope: "https://www.googleapis.com/auth/webmasters.readonly".to_string(),
        aud: "https://oauth2.googleapis.com/token".to_string(),
        exp: now + 3600,
        iat: now,
    };

    let encoding_key = EncodingKey::from_rsa_pem(key.private_key.as_bytes())
        .map_err(|e| Error::Other(format!("JWT key error: {}", e)))?;

    let jwt = encode(&Header::new(Algorithm::RS256), &claims, &encoding_key)
        .map_err(|e| Error::Other(format!("JWT encode error: {}", e)))?;

    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", jwt.as_str()),
        ])
        .send()
        .await?
        .json()
        .await?;

    let access_token = resp["access_token"]
        .as_str()
        .ok_or_else(|| Error::Other(format!("Token exchange failed: {}", resp)))?
        .to_string();
    let expires_in = resp["expires_in"].as_i64().unwrap_or(3600);

    Ok(TokenState {
        access_token,
        expires_at: Utc::now().timestamp() + expires_in,
    })
}

/// Resolve GSC service-account JSON path via [`EnvResolver`] precedence
/// (`secrets.env` → project `.env*` → shell).
///
/// Accepts `GSC_SERVICE_ACCOUNT_PATH` or `GOOGLE_APPLICATION_CREDENTIALS`.
pub fn resolve_service_account_path(project_path: &str) -> Option<String> {
    let resolver = crate::config::env_resolver::EnvResolver::new(project_path);
    resolver
        .resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
}

/// Prefer a non-empty injected access token; otherwise mint a short-lived
/// service-account token for the project.
///
/// CLI `execute-task` historically passed `None` (desktop injection removed).
/// Steps that need URL Inspection / Search Analytics must not soft-succeed
/// without a token — call this instead of treating missing inject as deferred.
pub async fn resolve_access_token(
    project_path: &str,
    injected: Option<&str>,
) -> Result<String> {
    if let Some(t) = injected.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(t.to_string());
    }
    let sa_path = resolve_service_account_path(project_path).ok_or_else(|| {
        Error::Other(
            "GSC_SERVICE_ACCOUNT_PATH not configured — set it in ~/.config/automation/secrets.env \
             (or project .env / GOOGLE_APPLICATION_CREDENTIALS)"
                .to_string(),
        )
    })?;
    Ok(get_service_account_token(&sa_path).await?.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;

    #[test]
    fn resolve_service_account_path_finds_project_env_or_machine_secrets() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "ps_gsc_auth_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let sa = dir.join("sa.json");
        std::fs::write(&sa, "{}").unwrap();
        std::fs::write(
            dir.join(".env"),
            format!("GSC_SERVICE_ACCOUNT_PATH={}", sa.display()),
        )
        .unwrap();

        let resolved = resolve_service_account_path(dir.to_str().unwrap());
        assert!(
            resolved.is_some(),
            "expected SA path from project .env and/or machine secrets.env"
        );
        // Machine secrets.env wins over project .env when both set — either is OK.
        let path = resolved.unwrap();
        assert!(
            path == sa.to_string_lossy()
                || std::path::Path::new(&path).exists()
                || path.contains("gsc")
                || path.ends_with(".json"),
            "unexpected SA path: {path}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn resolve_access_token_prefers_injected() {
        let token = resolve_access_token("/nonexistent-project-path", Some("injected-token"))
            .await
            .unwrap();
        assert_eq!(token, "injected-token");
    }

    #[tokio::test]
    async fn resolve_access_token_fails_without_sa_or_inject() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Isolated temp repo with no GSC keys in project env files. Machine
        // secrets.env may still win via EnvResolver — skip assert if present.
        let dir = std::env::temp_dir().join(format!(
            "ps_gsc_auth_none_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        if resolve_service_account_path(dir.to_str().unwrap()).is_some() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let err = resolve_access_token(dir.to_str().unwrap(), None)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("GSC_SERVICE_ACCOUNT_PATH"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ─── OAuth2 browser flow ──────────────────────────────────────────────────────

pub async fn start_oauth_flow(client_secrets_path: &str) -> Result<TokenState> {
    let content = std::fs::read_to_string(client_secrets_path)
        .map_err(|e| Error::Other(format!("Failed to read OAuth secrets: {}", e)))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| Error::Other(format!("Invalid OAuth secrets JSON: {}", e)))?;

    let app = json
        .get("installed")
        .or_else(|| json.get("web"))
        .ok_or_else(|| {
            Error::Other("Invalid OAuth secrets (need 'installed' or 'web' key)".to_string())
        })?;

    let client_id = app["client_id"]
        .as_str()
        .ok_or_else(|| Error::Other("Missing client_id".to_string()))?
        .to_string();
    let client_secret = app["client_secret"]
        .as_str()
        .ok_or_else(|| Error::Other("Missing client_secret".to_string()))?
        .to_string();

    let redirect_uri = "http://localhost:8085";
    let state_param = Utc::now().timestamp_millis().to_string();

    let scope =
        urlencoding::encode("https://www.googleapis.com/auth/webmasters.readonly").into_owned();
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&\
         response_type=code&scope={}&access_type=offline&state={}&prompt=consent",
        urlencoding::encode(&client_id),
        urlencoding::encode(redirect_uri),
        scope,
        state_param,
    );

    open_browser(&auth_url)?;

    // Wait for redirect callback
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:8085")
        .await
        .map_err(|e| Error::Other(format!("Cannot bind port 8085 (may be in use): {}", e)))?;

    let (mut stream, _) =
        tokio::time::timeout(std::time::Duration::from_secs(180), listener.accept())
            .await
            .map_err(|_| Error::Other("OAuth timed out after 3 minutes.".to_string()))?
            .map_err(|e| Error::Other(e.to_string()))?;

    let mut buf = vec![0u8; 8192];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| Error::Other(e.to_string()))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Extract code from GET /?code=...&state=... HTTP/1.1
    let code = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| path.split('?').nth(1))
        .and_then(|query| {
            query.split('&').find_map(|kv| {
                let mut parts = kv.splitn(2, '=');
                if parts.next() == Some("code") {
                    parts.next().map(String::from)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| Error::Other("No authorization code in callback.".to_string()))?;

    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
              <html><body style=\"font-family:sans-serif;padding:2rem\">\
              <h2>Authentication complete.</h2><p>Return to PageSeeds. You can close this tab.</p>\
              </body></html>",
        )
        .await
        .ok();
    drop(stream);

    // Exchange code for access token
    let http = reqwest::Client::new();
    let resp: serde_json::Value = http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?
        .json()
        .await?;

    let access_token = resp["access_token"]
        .as_str()
        .ok_or_else(|| Error::Other(format!("Token exchange failed: {}", resp)))?
        .to_string();
    let expires_in = resp["expires_in"].as_i64().unwrap_or(3600);

    Ok(TokenState {
        access_token,
        expires_at: Utc::now().timestamp() + expires_in,
    })
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| Error::Other(format!("Failed to open browser: {}", e)))?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|e| Error::Other(format!("Failed to open browser: {}", e)))?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn()
        .map_err(|e| Error::Other(format!("Failed to open browser: {}", e)))?;

    Ok(())
}
