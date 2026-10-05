use anyhow::Context;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::auth::BCRYPT_COST;

/// 32 random bytes encode to 64 hex chars, which stays under bcrypt's 72-byte
/// input limit so no part of the token is silently truncated when hashed.
const REFRESH_TOKEN_BYTES: usize = 32;
const REFRESH_TOKEN_HEX_LEN: usize = REFRESH_TOKEN_BYTES * 2;
const SECONDS_PER_MINUTE: i64 = 60;
const JWT_ALGORITHM: Algorithm = Algorithm::HS256;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub email: String,
    pub exp: usize, // expiry timestamp
    pub iat: usize, // issued at
}

pub fn generate_access_token(
    user_id: &str,
    email: &str,
    secret: &str,
    expiry_minutes: i64,
) -> anyhow::Result<String> {
    let issued_at = chrono::Utc::now().timestamp();
    let expires_at = issued_at
        .checked_add(expiry_minutes.saturating_mul(SECONDS_PER_MINUTE))
        .context("access token expiry overflow")?;
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        exp: usize::try_from(expires_at).context("invalid access token expiry")?,
        iat: usize::try_from(issued_at).context("invalid access token issue time")?,
    };
    encode(
        &Header::new(JWT_ALGORITHM),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("failed to sign access token")
}

pub fn verify_access_token(
    token: &str,
    secret: &str,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    let validation = Validation::new(JWT_ALGORITHM);
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation)
        .map(|data| data.claims)
}

/// Random 64-char lowercase hex string from the OS CSPRNG.
pub fn generate_refresh_token() -> String {
    let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn is_well_formed_refresh_token(token: &str) -> bool {
    token.len() == REFRESH_TOKEN_HEX_LEN && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// CPU-heavy: call from `spawn_blocking`.
pub fn hash_refresh_token(token: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(token, BCRYPT_COST)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "a-test-secret-that-is-at-least-32-chars";
    const OTHER_SECRET: &str = "another-test-secret-at-least-32-chars!!";
    const EXPIRY_MINUTES: i64 = 15;
    /// Well past jsonwebtoken's default 60s leeway.
    const EXPIRED_AGO_SECS: i64 = 3600;

    #[test]
    fn access_token_round_trips_claims() {
        let token = generate_access_token("u1", "a@b.c", SECRET, EXPIRY_MINUTES).unwrap();
        let claims = verify_access_token(&token, SECRET).unwrap();
        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.email, "a@b.c");
        assert_eq!(
            claims.exp - claims.iat,
            usize::try_from(EXPIRY_MINUTES * SECONDS_PER_MINUTE).unwrap()
        );
    }

    #[test]
    fn access_token_rejects_wrong_secret_and_tampering() {
        let token = generate_access_token("u1", "a@b.c", SECRET, EXPIRY_MINUTES).unwrap();
        assert!(verify_access_token(&token, OTHER_SECRET).is_err());
        assert!(verify_access_token(&format!("{token}x"), SECRET).is_err());
        assert!(verify_access_token("not-a-jwt", SECRET).is_err());
    }

    #[test]
    fn access_token_rejects_expired() {
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: "u1".into(),
            email: "a@b.c".into(),
            exp: usize::try_from(now - EXPIRED_AGO_SECS).unwrap(),
            iat: usize::try_from(now - 2 * EXPIRED_AGO_SECS).unwrap(),
        };
        let token = encode(
            &Header::new(JWT_ALGORITHM),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert!(verify_access_token(&token, SECRET).is_err());
    }

    #[test]
    fn refresh_tokens_are_random_hex_and_hash_verifies() {
        let token = generate_refresh_token();
        assert!(is_well_formed_refresh_token(&token));
        assert_ne!(token, generate_refresh_token());

        let hash = hash_refresh_token(&token).unwrap();
        assert_ne!(hash, token);
        assert!(bcrypt::verify(&token, &hash).unwrap());
        assert!(!bcrypt::verify(generate_refresh_token(), &hash).unwrap());
    }

    #[test]
    fn rejects_malformed_refresh_tokens() {
        assert!(!is_well_formed_refresh_token(""));
        assert!(!is_well_formed_refresh_token(&"g".repeat(REFRESH_TOKEN_HEX_LEN)));
        assert!(!is_well_formed_refresh_token(&"a".repeat(REFRESH_TOKEN_HEX_LEN + 1)));
    }
}
