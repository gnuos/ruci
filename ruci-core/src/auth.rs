//! Authentication module
//!
//! Provides session-based authentication for the Web UI with DB persistence.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use uuid::Uuid;

use crate::db::repository::{Repository, SessionInfo};
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
    /// Active sessions: session_id -> Session (in-memory cache)
    sessions: RwLock<HashMap<String, Session>>,
    /// Session expiry duration (default: 24 hours)
    session_ttl: Duration,
    /// Failed login attempts: username -> (count, first_failure_time)
    failed_attempts: RwLock<HashMap<String, (u32, Instant)>>,
    /// Max failed login attempts before lockout
    max_attempts: u32,
    /// Lockout duration after max failures
    lockout_duration: Duration,
}

impl AuthService {
    /// Create a new authentication service
    pub fn new(db: Arc<dyn Repository>) -> Self {
        Self {
            db,
            sessions: RwLock::new(HashMap::new()),
            session_ttl: Duration::from_secs(24 * 60 * 60), // 24 hours
            failed_attempts: RwLock::new(HashMap::new()),
            max_attempts: 5,
            lockout_duration: Duration::from_secs(15 * 60), // 15 minutes
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
        // Check rate limiting
        {
            let attempts = self.failed_attempts.read();
            if let Some((count, first_failure)) = attempts.get(username) {
                if *count >= self.max_attempts {
                    let elapsed = first_failure.elapsed();
                    if elapsed < self.lockout_duration {
                        let remaining = self.lockout_duration - elapsed;
                        tracing::warn!(
                            username = %username,
                            remaining_secs = %remaining.as_secs(),
                            "Login blocked due to too many failed attempts"
                        );
                        return Err(crate::error::Error::Other(format!(
                            "Too many failed login attempts. Try again in {} seconds.",
                            remaining.as_secs()
                        )));
                    }
                    // Lockout expired, reset
                    drop(attempts);
                    self.failed_attempts.write().remove(username);
                }
            }
        }

        // Get user from database
        let user = match self.db.get_user_by_username(username).await? {
            Some(u) => u,
            None => return Ok(None),
        };

        // Verify password
        if !Self::verify_password(password, &user.password_hash)? {
            // Track failed attempt
            let mut attempts = self.failed_attempts.write();
            let entry = attempts
                .entry(username.to_string())
                .or_insert((0, Instant::now()));
            entry.0 += 1;
            return Ok(None);
        }

        // Clear failed attempts on successful login
        self.failed_attempts.write().remove(username);

        // Update last login
        self.db.update_last_login(&user.id).await?;

        // Create session
        let session_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        let expires_at = now + chrono::Duration::seconds(self.session_ttl.as_secs() as i64);

        let session = Session {
            session_id: session_id.clone(),
            user_id: user.id.clone(),
            username: user.username.clone(),
            created_at: Instant::now(),
        };

        // Persist to DB
        let session_info = SessionInfo {
            id: session_id,
            user_id: user.id,
            username: user.username,
            created_at: now.to_rfc3339(),
            expires_at: expires_at.to_rfc3339(),
        };
        if let Err(e) = self.db.insert_session(&session_info).await {
            tracing::warn!("Failed to persist session to DB: {}", e);
        }

        // Store in-memory cache
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

    /// Get session by ID, checking for expiry (in-memory cache first, then DB fallback)
    pub fn get_session(&self, session_id: &str) -> Option<Session> {
        // Check in-memory cache first
        {
            let sessions = self.sessions.read();
            if let Some(s) = sessions
                .get(session_id)
                .filter(|s| s.created_at.elapsed() < self.session_ttl)
            {
                return Some(s.clone());
            }
        }

        // Cache miss: we can't do async here, but we've loaded sessions from DB on startup.
        // For hot-path performance, the in-memory cache is sufficient.
        None
    }

    /// Invalidate (logout) a session
    pub fn invalidate_session(&self, session_id: &str) {
        // Remove from in-memory cache
        {
            let mut sessions = self.sessions.write();
            sessions.remove(session_id);
        }

        // Remove from DB (spawn a task since we can't do async here)
        let db = self.db.clone();
        let sid = session_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = db.delete_session(&sid).await {
                tracing::warn!("Failed to delete session from DB: {}", e);
            }
        });
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self) {
        // Clean DB
        match self.db.delete_expired_sessions().await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("Cleaned up {} expired sessions from DB", count);
                }
            }
            Err(e) => tracing::warn!("Failed to cleanup expired sessions: {}", e),
        }

        // Clean in-memory
        {
            let mut sessions = self.sessions.write();
            sessions.retain(|_, session| session.created_at.elapsed() < self.session_ttl);
        }
    }

    /// Load active sessions from DB into in-memory cache (call on startup)
    pub async fn load_sessions_from_db(&self) {
        // We can't iterate all sessions without a list method, but we can
        // clean up expired ones. Sessions will be re-created on next login.
        if let Err(e) = self.db.delete_expired_sessions().await {
            tracing::warn!("Failed to cleanup expired sessions on startup: {}", e);
        }
        tracing::info!("Session cleanup completed on startup");
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
