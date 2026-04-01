//! Authentication module
//!
//! Provides session-based authentication for the Web UI.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use uuid::Uuid;

use crate::db::repository::Repository;
use crate::error::Result;

/// Session information
#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub created_at: Instant,
}

/// Authentication service for managing users and sessions
pub struct AuthService {
    /// Database repository for user operations
    db: Arc<dyn Repository>,
    /// Active sessions: session_id -> Session
    sessions: RwLock<HashMap<String, Session>>,
    /// Session expiry duration (default: 24 hours)
    session_ttl: Duration,
}

impl AuthService {
    /// Create a new authentication service
    pub fn new(db: Arc<dyn Repository>) -> Self {
        Self {
            db,
            sessions: RwLock::new(HashMap::new()),
            session_ttl: Duration::from_secs(24 * 60 * 60), // 24 hours
        }
    }

    /// Hash a password using bcrypt
    pub fn hash_password(password: &str) -> crate::error::Result<String> {
        bcrypt::hash(password, bcrypt::DEFAULT_COST)
            .map_err(|e| crate::error::Error::Other(e.to_string()))
    }

    /// Verify a password against a bcrypt hash
    pub fn verify_password(password: &str, hash: &str) -> crate::error::Result<bool> {
        bcrypt::verify(password, hash).map_err(|e| crate::error::Error::Other(e.to_string()))
    }

    /// Authenticate a user with username and password
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<Option<Session>> {
        // Get user from database
        let user = match self.db.get_user_by_username(username).await? {
            Some(u) => u,
            None => return Ok(None),
        };

        // Verify password
        if !Self::verify_password(password, &user.password_hash)? {
            return Ok(None);
        }

        // Update last login
        self.db.update_last_login(&user.id).await?;

        // Create session
        let session = Session {
            session_id: Uuid::new_v4().to_string(),
            user_id: user.id.clone(),
            username: user.username.clone(),
            created_at: Instant::now(),
        };

        // Store session
        {
            let mut sessions = self.sessions.write();
            sessions.insert(session.session_id.clone(), session.clone());
        }

        Ok(Some(session))
    }

    /// Validate a session and return the session if valid
    pub fn validate_session(&self, session_id: &str) -> Option<Session> {
        let sessions = self.sessions.read();
        sessions.get(session_id).cloned()
    }

    /// Get session by ID, checking for expiry
    pub fn get_session(&self, session_id: &str) -> Option<Session> {
        let sessions = self.sessions.read();
        sessions
            .get(session_id)
            .filter(|s| s.created_at.elapsed() < self.session_ttl)
            .cloned()
    }

    /// Invalidate (logout) a session
    pub fn invalidate_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write();
        sessions.remove(session_id);
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) {
        let mut sessions = self.sessions.write();
        sessions.retain(|_, session| session.created_at.elapsed() < self.session_ttl);
    }

    /// Get the number of active sessions
    pub fn active_sessions_count(&self) -> usize {
        let sessions = self.sessions.read();
        sessions.len()
    }

    /// Initialize the admin user if it doesn't exist
    pub async fn init_admin_user(&self, admin_username: &str, admin_password: &str) -> Result<()> {
        // Check if admin user exists
        if let Some(_) = self.db.get_user_by_username(admin_username).await? {
            tracing::info!("Admin user already exists, skipping initialization");
            return Ok(());
        }

        // Create admin user
        let password_hash = Self::hash_password(admin_password)?;
        let admin_user = crate::db::repository::UserInfo {
            id: Uuid::new_v4().to_string(),
            username: admin_username.to_string(),
            password_hash,
            role: "admin".to_string(),
            created_at: chrono::Utc::now().to_string(),
            last_login_at: None,
        };

        self.db.insert_user(&admin_user).await?;
        tracing::info!("Created admin user: {}", admin_username);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hash_and_verify() {
        let password = "test_password_123";
        let hash = AuthService::hash_password(password).unwrap();
        assert!(AuthService::verify_password(password, &hash).unwrap());
        assert!(!AuthService::verify_password("wrong_password", &hash).unwrap());
    }
}
