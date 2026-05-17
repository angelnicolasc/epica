//! OAuth 2.1 JWT Bearer authentication middleware.
//!
//! Supports two modes, selected via `EPICA_JWT_ALG` (default `hs256`):
//!
//! - **HS256** — shared-secret HMAC. For development only.
//!   Set `EPICA_JWT_SECRET` (default: a weak dev secret).
//! - **RS256** — RSA public-key verification. For production.
//!   Set `EPICA_JWT_RSA_PEM` (PEM string) or `EPICA_JWT_RSA_PEM_FILE` (path).
//!   The `/.well-known/jwks.json` endpoint then returns the real `n`/`e` components.
//!
//! Set `EPICA_NO_AUTH=1` to disable auth entirely (dev/test mode).

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Claims embedded in a valid Epica JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
    #[serde(default)]
    pub scope: Vec<String>,
}

/// JWT signing algorithm and associated key material.
#[derive(Debug, Clone)]
pub enum JwtAlgorithm {
    /// HMAC-SHA256 with a shared secret. Development only.
    Hs256 { secret: String },
    /// RSA-SHA256 with a public key. Enterprise/production.
    Rs256 { public_key_pem: String, kid: String },
}

/// Auth configuration built from environment variables at startup.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub algorithm: JwtAlgorithm,
    pub audience: String,
    pub issuer: String,
    /// When `true` (set via `EPICA_NO_AUTH=1`), all requests pass through.
    pub disabled: bool,
}

impl AuthConfig {
    /// Build `AuthConfig` from environment variables.
    ///
    /// Reads:
    /// - `EPICA_JWT_ALG` — `"hs256"` (default) or `"rs256"`
    /// - `EPICA_JWT_SECRET` — HS256 shared secret
    /// - `EPICA_JWT_RSA_PEM` — RS256 public key PEM (inline)
    /// - `EPICA_JWT_RSA_PEM_FILE` — RS256 public key PEM path
    /// - `EPICA_JWT_KID` — Key ID for RS256 JWKS (default `"epica-rs256-1"`)
    /// - `EPICA_JWT_AUDIENCE` — expected `aud` claim (default `"epica"`)
    /// - `EPICA_JWT_ISSUER` — expected `iss` claim (default `"epica"`)
    pub fn from_env() -> Self {
        let audience = std::env::var("EPICA_JWT_AUDIENCE").unwrap_or_else(|_| "epica".into());
        let issuer = std::env::var("EPICA_JWT_ISSUER").unwrap_or_else(|_| "epica".into());
        let disabled = std::env::var("EPICA_NO_AUTH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let alg_str = std::env::var("EPICA_JWT_ALG").unwrap_or_else(|_| "hs256".into());
        let algorithm = if alg_str.eq_ignore_ascii_case("rs256") {
            let pem = if let Ok(pem) = std::env::var("EPICA_JWT_RSA_PEM") {
                pem
            } else if let Ok(path) = std::env::var("EPICA_JWT_RSA_PEM_FILE") {
                std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read EPICA_JWT_RSA_PEM_FILE={path}: {e}"))
            } else {
                panic!("RS256 mode requires EPICA_JWT_RSA_PEM or EPICA_JWT_RSA_PEM_FILE to be set");
            };
            let kid = std::env::var("EPICA_JWT_KID").unwrap_or_else(|_| "epica-rs256-1".into());
            JwtAlgorithm::Rs256 { public_key_pem: pem, kid }
        } else {
            let secret = std::env::var("EPICA_JWT_SECRET")
                .unwrap_or_else(|_| "epica-dev-secret-change-in-production".into());
            JwtAlgorithm::Hs256 { secret }
        };

        Self { algorithm, audience, issuer, disabled }
    }

    /// Convenience constructor for tests and `--no-auth` mode.
    pub fn disabled() -> Self {
        Self {
            algorithm: JwtAlgorithm::Hs256 { secret: String::new() },
            audience: "epica".into(),
            issuer: "epica".into(),
            disabled: true,
        }
    }

    /// JWKS response body for `/.well-known/jwks.json`.
    ///
    /// HS256 mode: informational stub (symmetric key has no public component).
    /// RS256 mode: real JWK with `n` and `e` base64url-encoded components.
    pub fn jwks_response(&self) -> serde_json::Value {
        match &self.algorithm {
            JwtAlgorithm::Hs256 { .. } => json!({
                "keys": [{
                    "kty": "oct",
                    "use": "sig",
                    "alg": "HS256",
                    "kid": "epica-dev-1",
                    "_note": "HS256 symmetric dev key — no public component. Set EPICA_JWT_ALG=rs256 for production JWKS rotation."
                }]
            }),
            JwtAlgorithm::Rs256 { public_key_pem, kid } => {
                match extract_rsa_jwk(public_key_pem, kid) {
                    Ok(jwk) => json!({ "keys": [jwk] }),
                    Err(e) => {
                        tracing::error!("failed to serialize RS256 public key as JWK: {e}");
                        json!({ "keys": [], "error": format!("JWK serialization failed: {e}") })
                    }
                }
            }
        }
    }
}

/// Synchronous auth check used by the axum middleware closure in `server.rs`.
///
/// Returns `Ok(())` if the request is authorized or exempt.
/// Returns `Err(Response)` with appropriate HTTP status/body on failure.
pub fn check_auth(req: &Request, config: &AuthConfig) -> Result<(), Response> {
    if config.disabled {
        return Ok(());
    }

    let path = req.uri().path();
    let is_exempt = path.starts_with("/.well-known/")
        || path == "/health"
        || path == "/ready"
        || path == "/metrics";

    if is_exempt {
        return Ok(());
    }

    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "missing Authorization: Bearer <token> header" })),
            )
                .into_response()
        })?;

    validate_jwt(token, config).map(|_| ()).map_err(|e| {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": e }))).into_response()
    })
}

/// Axum `from_fn` middleware — validates JWT Bearer tokens.
///
/// Exempt paths (no token required):
/// - `/.well-known/*` — discovery
/// - `/health`, `/ready`, `/metrics` — infrastructure probes
pub async fn auth_middleware(req: Request, next: Next) -> Response {
    let config = req
        .extensions()
        .get::<AuthConfig>()
        .cloned()
        .unwrap_or_else(AuthConfig::disabled);

    match check_auth(&req, &config) {
        Ok(()) => next.run(req).await,
        Err(resp) => resp,
    }
}

fn validate_jwt(token: &str, config: &AuthConfig) -> Result<JwtClaims, String> {
    match &config.algorithm {
        JwtAlgorithm::Hs256 { secret } => {
            let key = DecodingKey::from_secret(secret.as_bytes());
            let mut v = Validation::new(Algorithm::HS256);
            v.set_audience(&[&config.audience]);
            v.set_issuer(&[&config.issuer]);
            decode::<JwtClaims>(token, &key, &v)
                .map(|d| d.claims)
                .map_err(|e| format!("invalid JWT: {e}"))
        }
        JwtAlgorithm::Rs256 { public_key_pem, .. } => {
            let key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
                .map_err(|e| format!("invalid RSA public key PEM: {e}"))?;
            let mut v = Validation::new(Algorithm::RS256);
            v.set_audience(&[&config.audience]);
            v.set_issuer(&[&config.issuer]);
            decode::<JwtClaims>(token, &key, &v)
                .map(|d| d.claims)
                .map_err(|e| format!("invalid JWT: {e}"))
        }
    }
}

/// Parse a public key PEM (PKCS#8 SPKI or PKCS#1) and return a JWK object.
fn extract_rsa_jwk(pem: &str, kid: &str) -> Result<serde_json::Value, String> {
    use rsa::pkcs8::DecodePublicKey;

    let public_key = rsa::RsaPublicKey::from_public_key_pem(pem)
        .or_else(|_| {
            use rsa::pkcs1::DecodeRsaPublicKey;
            rsa::RsaPublicKey::from_pkcs1_pem(pem)
        })
        .map_err(|e| format!("failed to parse RSA public key PEM: {e}"))?;

    let n = base64url(&public_key.n().to_bytes_be());
    let e = base64url(&public_key.e().to_bytes_be());

    Ok(json!({
        "kty": "RSA",
        "use": "sig",
        "alg": "RS256",
        "kid": kid,
        "n": n,
        "e": e,
    }))
}

/// Base64url encoding without padding (RFC 4648 §5).
fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[b2 & 0x3f] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    fn now_plus(secs: u64) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + secs
    }

    fn test_claims(exp: u64) -> JwtClaims {
        JwtClaims { sub: "test-agent".into(), exp, iat: 0, scope: vec![] }
    }

    fn hs256_cfg(secret: &str) -> AuthConfig {
        AuthConfig {
            algorithm: JwtAlgorithm::Hs256 { secret: secret.into() },
            audience: "epica".into(),
            issuer: "epica".into(),
            disabled: false,
        }
    }

    fn hs256_token(secret: &str, claims: &JwtClaims) -> String {
        let key = EncodingKey::from_secret(secret.as_bytes());
        encode(&Header::new(Algorithm::HS256), claims, &key).unwrap()
    }

    #[test]
    fn hs256_valid_token_passes() {
        let cfg = hs256_cfg("super-secret");
        let token = hs256_token("super-secret", &test_claims(now_plus(3600)));
        assert!(validate_jwt(&token, &cfg).is_ok());
    }

    #[test]
    fn hs256_wrong_secret_fails() {
        let cfg = hs256_cfg("correct-secret");
        let token = hs256_token("wrong-secret", &test_claims(now_plus(3600)));
        assert!(validate_jwt(&token, &cfg).is_err());
    }

    // ── RS256 tests — generate a 1024-bit key pair at test time ──────────────

    fn rs256_keypair() -> (rsa::RsaPrivateKey, rsa::RsaPublicKey) {
        use rand::rngs::OsRng;
        let private_key = rsa::RsaPrivateKey::new(&mut OsRng, 1024).unwrap();
        let public_key = rsa::RsaPublicKey::from(&private_key);
        (private_key, public_key)
    }

    fn rs256_cfg(public_key: &rsa::RsaPublicKey) -> AuthConfig {
        use rsa::pkcs8::EncodePublicKey;
        let pem = public_key.to_public_key_pem(Default::default()).unwrap();
        AuthConfig {
            algorithm: JwtAlgorithm::Rs256 {
                public_key_pem: pem.to_string(),
                kid: "test-rs256".into(),
            },
            audience: "epica".into(),
            issuer: "epica".into(),
            disabled: false,
        }
    }

    fn rs256_token(private_key: &rsa::RsaPrivateKey, claims: &JwtClaims) -> String {
        use rsa::pkcs8::EncodePrivateKey;
        let pem = private_key.to_pkcs8_pem(Default::default()).unwrap();
        let key = EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap();
        encode(&Header::new(Algorithm::RS256), claims, &key).unwrap()
    }

    #[test]
    fn rs256_valid_token_passes() {
        let (private_key, public_key) = rs256_keypair();
        let cfg = rs256_cfg(&public_key);
        let token = rs256_token(&private_key, &test_claims(now_plus(3600)));
        assert!(validate_jwt(&token, &cfg).is_ok());
    }

    #[test]
    fn rs256_wrong_key_fails() {
        let (signing_key, _) = rs256_keypair();
        let (_, wrong_public) = rs256_keypair();
        let cfg = rs256_cfg(&wrong_public);
        let token = rs256_token(&signing_key, &test_claims(now_plus(3600)));
        assert!(validate_jwt(&token, &cfg).is_err());
    }

    #[test]
    fn jwks_response_rs256_has_n_and_e() {
        let (_, public_key) = rs256_keypair();
        let cfg = rs256_cfg(&public_key);
        let jwks = cfg.jwks_response();
        let keys = jwks["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["kty"], "RSA");
        assert_eq!(keys[0]["alg"], "RS256");
        assert!(keys[0]["n"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
        assert!(keys[0]["e"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    }
}
