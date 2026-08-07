//! JWT-based authentication for REST API
//!
//! Provides access token / refresh token flow with RBAC support.

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{
    decode, encode, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Token type: "access" or "refresh"
    pub typ: String,
    /// User roles for RBAC
    pub roles: Vec<String>,
    /// Project IDs this token has access to
    pub project_ids: Vec<String>,
    /// JWT ID for revocation
    pub jti: String,
}

/// Token pair (access + refresh)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub token_type: String,
}

/// JWT service for token management
pub struct JwtService {
    config: JwtConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    /// In-memory refresh token store (in production, use Redis)
    refresh_store: parking_lot::RwLock<HashMap<String, RefreshTokenEntry>>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // Fields are written for future audit/inspection use
struct RefreshTokenEntry {
    user_id: String,
    roles: Vec<String>,
    project_ids: Vec<String>,
    created_at: DateTime<Utc>,
    revoked: bool,
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
            refresh_store: parking_lot::RwLock::new(HashMap::new()),
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

    /// Create refresh token
    pub fn create_refresh_token(
        &self,
        user_id: &str,
        roles: Vec<String>,
        project_ids: Vec<String>,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.config.refresh_ttl_secs);
        let jti = Uuid::new_v4().to_string();

        let claims = Claims {
            sub: user_id.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            typ: "refresh".to_string(),
            roles: roles.clone(),
            project_ids: project_ids.clone(),
            jti: jti.clone(),
        };

        let token = encode(&Header::default(), &claims, &self.encoding_key)?;

        // Store refresh token
        self.refresh_store.write().insert(
            jti.clone(),
            RefreshTokenEntry {
                user_id: user_id.to_string(),
                roles,
                project_ids,
                created_at: now,
                revoked: false,
            },
        );

        Ok(token)
    }

    /// Create token pair (access + refresh)
    pub fn create_token_pair(
        &self,
        user_id: &str,
        roles: Vec<String>,
        project_ids: Vec<String>,
    ) -> Result<TokenPair, jsonwebtoken::errors::Error> {
        let access_token = self.create_access_token(user_id, roles.clone(), project_ids.clone())?;
        let refresh_token = self.create_refresh_token(user_id, roles, project_ids)?;

        Ok(TokenPair {
            access_token,
            refresh_token,
            expires_in: self.config.access_ttl_secs,
            token_type: "Bearer".to_string(),
        })
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

    /// Validate and decode refresh token
    pub fn validate_refresh_token(
        &self,
        token: &str,
    ) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.set_issuer(&[&self.config.issuer]);

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;

        // Verify token type
        if token_data.claims.typ != "refresh" {
            return Err(jsonwebtoken::errors::ErrorKind::InvalidToken.into());
        }

        // Check if refresh token is revoked
        let jti = &token_data.claims.jti;
        let store = self.refresh_store.read();
        if let Some(entry) = store.get(jti) {
            if entry.revoked {
                return Err(jsonwebtoken::errors::ErrorKind::InvalidToken.into());
            }
        } else {
            return Err(jsonwebtoken::errors::ErrorKind::InvalidToken.into());
        }

        Ok(token_data)
    }

    /// Refresh access token using refresh token
    pub fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<TokenPair, jsonwebtoken::errors::Error> {
        let token_data = self.validate_refresh_token(refresh_token)?;
        let claims = token_data.claims;

        // Revoke old refresh token
        if let Some(entry) = self.refresh_store.write().get_mut(&claims.jti) {
            entry.revoked = true;
        }

        // Create new token pair
        self.create_token_pair(&claims.sub, claims.roles, claims.project_ids)
    }

    /// Revoke refresh token (logout)
    pub fn revoke_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<(), jsonwebtoken::errors::Error> {
        let token_data = self.validate_refresh_token(refresh_token)?;
        if let Some(entry) = self.refresh_store.write().get_mut(&token_data.claims.jti) {
            entry.revoked = true;
        }
        Ok(())
    }

    /// Check if user has required role
    pub fn has_role(&self, claims: &Claims, required_role: &str) -> bool {
        claims.roles.contains(&required_role.to_string())
            || claims.roles.contains(&"admin".to_string())
    }

    /// Check if user has access to project
    pub fn has_project_access(&self, claims: &Claims, project_id: &str) -> bool {
        claims.project_ids.contains(&project_id.to_string())
            || claims.project_ids.contains(&"*".to_string())
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
            refresh_ttl_secs: 604800,
            issuer: "sisyphus-bridge".to_string(),
        }
    }

    #[test]
    fn test_token_pair_creation() {
        let config = test_jwt_config();
        let service = JwtService::new(config).unwrap();

        let pair = service
            .create_token_pair("user1", vec!["user".to_string()], vec!["proj1".to_string()])
            .unwrap();

        assert!(!pair.access_token.is_empty());
        assert!(!pair.refresh_token.is_empty());
        assert_eq!(pair.token_type, "Bearer");
        assert_eq!(pair.expires_in, 900);
    }

    #[test]
    fn test_access_token_validation() {
        let config = test_jwt_config();
        let service = JwtService::new(config).unwrap();

        let pair = service
            .create_token_pair("user1", vec!["admin".to_string()], vec!["*".to_string()])
            .unwrap();

        let claims = service.validate_access_token(&pair.access_token).unwrap();
        assert_eq!(claims.claims.sub, "user1");
        assert!(claims.claims.roles.contains(&"admin".to_string()));
        assert_eq!(claims.claims.typ, "access");
    }

    #[test]
    fn test_refresh_token_flow() {
        let config = test_jwt_config();
        let service = JwtService::new(config).unwrap();

        let pair = service
            .create_token_pair("user1", vec!["user".to_string()], vec!["proj1".to_string()])
            .unwrap();

        // Wait a bit to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        let new_pair = service.refresh_access_token(&pair.refresh_token).unwrap();

        assert!(!new_pair.access_token.is_empty());
        assert!(!new_pair.refresh_token.is_empty());
        assert_ne!(pair.access_token, new_pair.access_token);
        assert_ne!(pair.refresh_token, new_pair.refresh_token);
    }

    #[test]
    fn test_role_check() {
        let config = test_jwt_config();
        let service = JwtService::new(config).unwrap();

        let claims = Claims {
            sub: "user1".to_string(),
            iat: 0,
            exp: 0,
            typ: "access".to_string(),
            roles: vec!["user".to_string(), "developer".to_string()],
            project_ids: vec!["proj1".to_string()],
            jti: "test".to_string(),
        };

        assert!(service.has_role(&claims, "user"));
        assert!(service.has_role(&claims, "developer"));
        assert!(!service.has_role(&claims, "admin"));
        // admin role grants all
        let admin_claims = Claims {
            roles: vec!["admin".to_string()],
            ..claims.clone()
        };
        assert!(service.has_role(&admin_claims, "any_role"));
    }

    #[test]
    fn test_project_access_check() {
        let config = test_jwt_config();
        let service = JwtService::new(config).unwrap();

        let claims = Claims {
            sub: "user1".to_string(),
            iat: 0,
            exp: 0,
            typ: "access".to_string(),
            roles: vec!["user".to_string()],
            project_ids: vec!["proj1".to_string(), "proj2".to_string()],
            jti: "test".to_string(),
        };

        assert!(service.has_project_access(&claims, "proj1"));
        assert!(service.has_project_access(&claims, "proj2"));
        assert!(!service.has_project_access(&claims, "proj3"));

        // wildcard access
        let admin_claims = Claims {
            project_ids: vec!["*".to_string()],
            ..claims
        };
        assert!(service.has_project_access(&admin_claims, "any_project"));
    }

    #[test]
    fn test_new_with_empty_secret_fails() {
        let config = JwtConfig {
            secret: String::new(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 604800,
            issuer: "sisyphus-bridge".to_string(),
        };
        let result = JwtService::new(config);
        assert!(result.is_err());
    }
}
