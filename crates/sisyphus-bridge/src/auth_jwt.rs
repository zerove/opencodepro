//! JWT-based authentication for REST API
//!
//! Provides access-token issuance and validation.

use chrono::{Duration, Utc};
use jsonwebtoken::{
    decode, encode, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sisyphus_types::config::JwtConfig;

/// JWT claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Issued at
    pub iat: i64,
    /// Expiration time
    pub exp: i64,
    /// Token type ("access")
    pub typ: String,
    /// User roles for RBAC
    pub roles: Vec<String>,
    /// Project IDs this token has access to
    pub project_ids: Vec<String>,
    /// Unique token ID
    pub jti: String,
}

/// JWT service for token management
pub struct JwtService {
    config: JwtConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtService {
    /// Create new JWT service
    pub fn new(config: JwtConfig) -> Result<Self, jsonwebtoken::errors::Error> {
        if config.secret.is_empty() {
            return Err(jsonwebtoken::errors::ErrorKind::InvalidKeyFormat.into());
        }
        let encoding_key = EncodingKey::from_secret(config.secret.as_bytes());
        let decoding_key = DecodingKey::from_secret(config.secret.as_bytes());
        Ok(Self {
            config,
            encoding_key,
            decoding_key,
        })
    }

    /// Create access token
    pub fn create_access_token(
        &self,
        user_id: &str,
        roles: Vec<String>,
        project_ids: Vec<String>,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.config.access_ttl_secs);
        let jti = Uuid::new_v4().to_string();

        let claims = Claims {
            sub: user_id.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            typ: "access".to_string(),
            roles,
            project_ids,
            jti,
        };

        encode(&Header::default(), &claims, &self.encoding_key)
    }

    /// Validate and decode access token
    pub fn validate_access_token(
        &self,
        token: &str,
    ) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.set_issuer(&[&self.config.issuer]);

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;

        // Verify token type
        if token_data.claims.typ != "access" {
            return Err(jsonwebtoken::errors::ErrorKind::InvalidToken.into());
        }

        Ok(token_data)
    }
}

/// Extract Bearer token from Authorization header
pub fn extract_bearer_token(auth_header: &str) -> Option<String> {
    auth_header.strip_prefix("Bearer ").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::config::JwtConfig;

    fn test_jwt_config() -> JwtConfig {
        JwtConfig {
            secret: "test-secret-key-for-testing-only".to_string(),
            access_ttl_secs: 900,
            issuer: "sisyphus-bridge".to_string(),
        }
    }

    #[test]
    fn test_access_token_validation() {
        let config = test_jwt_config();
        let service = JwtService::new(config).unwrap();

        let token = service
            .create_access_token("user1", vec!["admin".to_string()], vec!["*".to_string()])
            .unwrap();

        let claims = service.validate_access_token(&token).unwrap();
        assert_eq!(claims.claims.sub, "user1");
        assert!(claims.claims.roles.contains(&"admin".to_string()));
        assert_eq!(claims.claims.typ, "access");
    }

    #[test]
    fn test_new_with_empty_secret_fails() {
        let config = JwtConfig {
            secret: String::new(),
            access_ttl_secs: 900,
            issuer: "sisyphus-bridge".to_string(),
        };
        let result = JwtService::new(config);
        assert!(result.is_err());
    }
}
