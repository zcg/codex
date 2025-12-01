//! Token storage and retrieval.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::Code88Error;

/// File name for storing the 88code token.
const TOKEN_FILE_NAME: &str = "88code-token.json";

/// Structure for storing token data.
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenFile {
    /// The authentication token.
    pub token: String,
    /// When the token was obtained.
    pub created_at: DateTime<Utc>,
    /// How the token was obtained (e.g., "browser_login", "manual_input").
    #[serde(default)]
    pub source: String,
}

/// Get the path to the token file.
pub fn token_path(codex_home: &Path) -> PathBuf {
    codex_home.join(TOKEN_FILE_NAME)
}

/// Load token from the config directory.
///
/// Returns `None` if the token file doesn't exist or is invalid.
pub fn load_token(codex_home: &Path) -> Option<String> {
    let path = token_path(codex_home);

    if !path.exists() {
        debug!("Token file does not exist: {:?}", path);
        return None;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to read token file: {}", e);
            return None;
        }
    };

    let file: TokenFile = match serde_json::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to parse token file: {}", e);
            return None;
        }
    };

    if file.token.is_empty() {
        debug!("Token file contains empty token");
        return None;
    }

    Some(file.token)
}

/// Save token to the config directory.
///
/// Creates the directory if it doesn't exist.
/// Sets restrictive file permissions on Unix systems.
pub fn save_token(codex_home: &Path, token: &str) -> Result<(), Code88Error> {
    save_token_with_source(codex_home, token, "browser_login")
}

/// Save token with a specific source identifier.
pub fn save_token_with_source(
    codex_home: &Path,
    token: &str,
    source: &str,
) -> Result<(), Code88Error> {
    // Ensure directory exists
    std::fs::create_dir_all(codex_home)?;

    let file = TokenFile {
        token: token.to_string(),
        created_at: Utc::now(),
        source: source.to_string(),
    };

    let content = serde_json::to_string_pretty(&file)?;
    let path = token_path(codex_home);

    // Write with restrictive permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600);
        let mut file = options.open(&path)?;
        std::io::Write::write_all(&mut file, content.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(&path, &content)?;
    }

    debug!("Token saved to: {:?}", path);
    Ok(())
}

/// Delete the token file.
pub fn delete_token(codex_home: &Path) -> Result<(), Code88Error> {
    let path = token_path(codex_home);
    if path.exists() {
        std::fs::remove_file(&path)?;
        debug!("Token file deleted: {:?}", path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_token() {
        let dir = tempdir().unwrap();
        let token = "test_token_12345";

        save_token(dir.path(), token).unwrap();
        let loaded = load_token(dir.path());

        assert_eq!(loaded, Some(token.to_string()));
    }

    #[test]
    fn test_load_nonexistent_token() {
        let dir = tempdir().unwrap();
        let loaded = load_token(dir.path());
        assert_eq!(loaded, None);
    }

    #[test]
    fn test_delete_token() {
        let dir = tempdir().unwrap();
        save_token(dir.path(), "test").unwrap();
        assert!(token_path(dir.path()).exists());

        delete_token(dir.path()).unwrap();
        assert!(!token_path(dir.path()).exists());
    }
}
