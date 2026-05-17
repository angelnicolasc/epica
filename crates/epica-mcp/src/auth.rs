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
    use rsa::traits::PublicKeyParts;

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

    // ── RS256 tests — hardcoded 2048-bit test key pairs ──────────────────────
    // Pre-generated offline; parsing is instant vs. runtime generation (minutes in
    // debug mode due to primality search). These are TEST-ONLY keys — not secrets.

    /// Test keypair 1: private key (PKCS#8 PEM).
    const TEST_KEY1_PRIV: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCtbktWScvzqg6X\nzhsYaP5riMXu54FHT8r4ly3SrsgGtRxYHF0ICYj+wUBcWHWjLCzkMCueQtaczCYP\n51eTixk2JnePQta/1Rxx0eOzMXNpFrziE3IQTXwUkHiWgJHjxPg4o05T2QVnbWLv\nTI8dAglcEPSBiVa3/GlUnmtWRlkxLBJbOaU4MR++CqqqmEGkEnirZ8LwrDse2yMV\nRm+6TeKuNGKSwWED/x9gUl/81p0P6VPYxiPXuRijLT24V+73f/bFR1l+hUHBVhFI\nqAQhJEfxfKC/RfBOf2jTQNaKAXLcCswIL+ZZcjzqUYvVy4QQAoVE3mCoMxJdAcM/\nmAJdqXi5AgMBAAECggEAcLFfgMVZIo7ZBqllj9oBoCxyuUdzCLx/nkLWArWRwlID\nBfoANY3EmA1I3fiZEBtPXEM0xJSX0bER9nmTvYrAKiCaxdtfoa0/23HQLIswfBPL\nTnfmQVOoEdDCmsEWi1NdG6h56B/30/oPNIGh6O5+2HUn+9gbIliAtPxvsNLrd/gh\nW+wQzMMpy7GghqFU9hq7pqI055OVzRBS+6krvrpYdF7Tfw0BVdz0YUMta7wuTXkI\nuTBMeqfOQnw1hxXXqmPDOnpL/NL0dXm4hXqEc2tLtqBVB+baMGxNW31j9hR3kk2I\nE7BXsqandV7Dc9tVxxZEayPIUBnhSALkglAqqlzL0QKBgQDmFy4CdhKPV1FooI2L\nL/WsHaQTUC7fAhXpgBxXwM+ZKvCbAuzvUQbvpUc7VBVXgZL7OugXdLMIlJjJrSvF\nhrojMj1AffDe5c20vjnRdHrWRc+nca/fqG6ZzfAsgeH3ub/Wzyp20zJ1D72MpjAN\nQUNhWz0y1z5reF0LaKc+zmzczQKBgQDA9cj85VXDriZcBoIIEz6zC3QuPhiuzwwB\nDntRlV14IK+3iTCtJc43+iSZCuCOK8tuW+QV/95BShjVJU7ti+Zt3T+Svx7lRMHK\nF95zg9PGuMxsDijUU9yCiBdGkAEmJpKHUzeD4qr9Peq7Xc6WkFjQvbC3S0hcH2ek\n493/AQ5LnQKBgGpfKPwmTepKue4e25EPeQo7IdFz7ldXBX5PpcrD7rWm7lkbfyIc\nWZKM3GOHOd6cnrDayNWfM+2xlPkXv/avlHoVDdA06RiDMRhwIRa+PNO2rouAuYgy\nu/8LAA/zc94s142ddMo+VUNdJYpSgkB+fYISxjYs4ESa/pj5pugYUqe5AoGBAJQj\nnjxZrPBf4N9Bt86PR9GZd4aQ8c4y8ppVDePicjHplj2nu5EStzFOf45nRWKgyLtf\nHMqu92jUhCAPVnsUrsGl3ErDI+sMUGLg1E2G5a1o7rf+XuYzw9UKuiPYJqmtb00p\nXDOKb4+gW3ehWxtIkocfOm5eA52GFsIGlsZRfzIZAoGBALAHORgZxq3HX+02f3M9\n9vaXhdlaJhqgqeRRToeXPqXvFHXY/i7wKOuJAarQXB3tIeiugljW0elPvzif5lLW\nPKjmcCPs9DM4GKU8BGdkw/IsM6+NdIWDWU0DYpEPtjQgfcxX4gH4sn7eVgOnD9il\nvdyBC67ej7xWRcCa4olJa3xK\n-----END PRIVATE KEY-----";

    /// Test keypair 1: public key (SPKI PEM).
    const TEST_KEY1_PUB: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEArW5LVknL86oOl84bGGj+\na4jF7ueBR0/K+Jct0q7IBrUcWBxdCAmI/sFAXFh1oyws5DArnkLWnMwmD+dXk4sZ\nNiZ3j0LWv9UccdHjszFzaRa84hNyEE18FJB4loCR48T4OKNOU9kFZ21i70yPHQIJ\nXBD0gYlWt/xpVJ5rVkZZMSwSWzmlODEfvgqqqphBpBJ4q2fC8Kw7HtsjFUZvuk3i\nrjRiksFhA/8fYFJf/NadD+lT2MYj17kYoy09uFfu93/2xUdZfoVBwVYRSKgEISRH\n8Xygv0XwTn9o00DWigFy3ArMCC/mWXI86lGL1cuEEAKFRN5gqDMSXQHDP5gCXal4\nuQIDAQAB\n-----END PUBLIC KEY-----";

    /// Test keypair 2: private key (PKCS#8 PEM) — used as the "wrong key" in mismatch tests.
    const TEST_KEY2_PRIV: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDgq1TKyHYw5XuB\nnVPOGgs9VsxgGYIJrsW3etbxaeP5SCCRN2az3WS2upXG420dz/Nh4dxSSud2Kvez\nVia2OR74ti5gd0bBix9/KdzcC6pdvq4J4g6IaOEt7Ql3qr9cfr3HbFZoBKo5TglO\nn8cNMZM/xeLfxYieYTnRQR0X5YSV88t/m3558XLJNXvYfuivPke334VcOvty6H6m\nDkQJ7s11s1mr7g3+4xwDXKytreer3Odlxow+u5fwWvqGWTMs5BQEsEceKXX+OE33\nQEJLCv0cSmpo3BRHN89qJx+CJPg5xKBgByuG3sUia/aDeZTPcD2SpcZbbukkADCE\nwghhrVhlAgMBAAECggEBAIeUPRYmhNSbF74vMAy3QMMiZzEzE3s+YgiIc7+51B5x\n/V1E3pB6cTWoQYyFYCrWfBw8jZWHqEhyQ4qQ2cmrjNowLqp+ME/J4hb+L08HJydt\nU1+ZcIW3LPRnEAiMHPD3dxUqdrZM4mC0i/9LgnaezSp2A6Rgc0KIj7iMn770/d7y\n3/2zxnk0Q4PkTzkQV+38rKjV3X2rMy6rIO3AUGtm/+FW4sY53BYXnio3kstCuWvG\nfEJHmQKfWO+cYedo8pvifgfIuea1WfmhcPzIenOS6JMwLeDhNzVViktfnrsx12JX\n3y8eWUXXvqjZiGLo90aAkpByPNpkdNqDtNsnurm9/X0CgYEA9PgYLd7W4bUXX3t1\nKxsd7uVXe2jo+pSiJv0esIV5cp7yMnN93dBaXdARcGqvXBWDKsYA910yokj5OLR5\nFw9HuweWVwo7yWxdNUu4u9jz/D121Z2v1G87/RWsUyE/pRyClGm8+Jpq9VUn8X45\ntAz23J+5+Hxbo+OXaupgLRYY9PMCgYEA6sk6aj0wYEIim9iuacvyCPmPYjMdEyRw\nnRVbUwxgGipFO9LAAtHFkWOT2xCAbyecgWKIJq1SVJmNsuhMfkrgKmT0lzoUGzMa\nBCfcA/grPKrOAe8pendHaLX9P2k8W00pssv6nhbPvN730xkWQWedJeIAhAr+EeUn\n76O2Zql3M0cCgYEArfblzOV8eitNXuxgx+zo8/eAic517UXSZZfJzJftKF4CJ5vm\n3bgSBJ83UzsgL2fDj4OvuftAcwkZm5Bmkd6zFPoNZOCKlr9S7f9JQHWQxyerFYZ3\nEIix9EgI6bwp44p8nQL+RRn8LR99Tz1RozC1uvXfbrx5o8iDhlTNWhdgP8sCgYBs\nBPjjOBOxtbvGiAJ2mmZYyri1LV8LF5DYNKM3qlHst9XymBvPMEP9iBrWhtkQSuEu\nhe6uHL/sPFl9HnNTB4/q8Ve22/m0KeamUtBe4ybBWrQ9H5OtzIMGIfTJ39jtCKtO\nn5pGcahR9SN/8+LRZKJgc4JZPdV21j9xeZjJ0t4MsQKBgBGfdxnZo5APThNciYJy\njmH6YX5e4BGNTd2oVzxVNk3wcXohLabSjhrm432Bj730lOH/10nJmhMHCJD1SpHn\n0NIeW2Okgv/5+y/NrtL1JVJdZaD1J9DQ1dremoryz5xIuS/2YgZN2FftIUfk5RcD\nYRHfEfH34oS5k+vMMO7ftMKb\n-----END PRIVATE KEY-----";

    /// Test keypair 2: public key (SPKI PEM) — used as the "wrong" verifying key.
    const TEST_KEY2_PUB: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA4KtUysh2MOV7gZ1TzhoL\nPVbMYBmCCa7Ft3rW8Wnj+UggkTdms91ktrqVxuNtHc/zYeHcUkrndir3s1Ymtjke\n+LYuYHdGwYsffync3AuqXb6uCeIOiGjhLe0Jd6q/XH69x2xWaASqOU4JTp/HDTGT\nP8Xi38WInmE50UEdF+WElfPLf5t+efFyyTV72H7orz5Ht9+FXDr7cuh+pg5ECe7N\ndbNZq+4N/uMcA1ysra3nq9znZcaMPruX8Fr6hlkzLOQUBLBHHil1/jhN90BCSwr9\nHEpqaNwURzfPaicfgiT4OcSgYAcrht7FImv2g3mUz3A9kqXGW27pJAAwhMIIYa1Y\nZQIDAQAB\n-----END PUBLIC KEY-----";

    fn rs256_cfg_from_pub_pem(pub_pem: &str) -> AuthConfig {
        AuthConfig {
            algorithm: JwtAlgorithm::Rs256 {
                public_key_pem: pub_pem.to_string(),
                kid: "test-rs256".into(),
            },
            audience: "epica".into(),
            issuer: "epica".into(),
            disabled: false,
        }
    }

    fn rs256_token_from_priv_pem(priv_pem: &str, claims: &JwtClaims) -> String {
        let key = EncodingKey::from_rsa_pem(priv_pem.as_bytes()).unwrap();
        encode(&Header::new(Algorithm::RS256), claims, &key).unwrap()
    }

    #[test]
    fn rs256_valid_token_passes() {
        let cfg = rs256_cfg_from_pub_pem(TEST_KEY1_PUB);
        let token = rs256_token_from_priv_pem(TEST_KEY1_PRIV, &test_claims(now_plus(3600)));
        assert!(validate_jwt(&token, &cfg).is_ok());
    }

    #[test]
    fn rs256_wrong_key_fails() {
        // Sign with key 1, verify with key 2's public key → must fail.
        let cfg = rs256_cfg_from_pub_pem(TEST_KEY2_PUB);
        let token = rs256_token_from_priv_pem(TEST_KEY1_PRIV, &test_claims(now_plus(3600)));
        assert!(validate_jwt(&token, &cfg).is_err());
    }

    #[test]
    fn jwks_response_rs256_has_n_and_e() {
        let cfg = rs256_cfg_from_pub_pem(TEST_KEY1_PUB);
        let jwks = cfg.jwks_response();
        let keys = jwks["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["kty"], "RSA");
        assert_eq!(keys[0]["alg"], "RS256");
        assert!(keys[0]["n"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
        assert!(keys[0]["e"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    }
}
