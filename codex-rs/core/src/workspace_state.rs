use crate::protocol_config_types::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use sha1::Digest;
use sha1::Sha1;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tracing::warn;

const WORKSPACE_STATE_DIR: &str = "workspace_state";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub model: Option<String>,
    pub model_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, WorkspaceMcpServerState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceMcpServerState {
    pub enabled: Option<bool>,
}

fn workspace_state_path(codex_home: &Path, workspace: &Path) -> PathBuf {
    let canonical = dunce::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut hasher = Sha1::new();
    hasher.update(canonical.as_os_str().to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let filename = format!("{digest:x}.toml");
    codex_home.join(WORKSPACE_STATE_DIR).join(filename)
}

pub fn load_workspace_state(
    codex_home: &Path,
    workspace: &Path,
) -> std::io::Result<WorkspaceState> {
    let path = workspace_state_path(codex_home, workspace);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorkspaceState::default());
        }
        Err(err) => return Err(err),
    };

    match toml::from_str::<WorkspaceState>(&contents) {
        Ok(state) => Ok(state),
        Err(err) => {
            warn!(
                "Failed to parse workspace state from {}: {err}",
                path.display()
            );
            Ok(WorkspaceState::default())
        }
    }
}

fn persist_workspace_state(
    codex_home: &Path,
    workspace: &Path,
    mut state: WorkspaceState,
) -> std::io::Result<()> {
    // Avoid storing empty MCP server entries with no data.
    state.mcp_servers.retain(|_, entry| entry.enabled.is_some());

    let path = workspace_state_path(codex_home, workspace);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut temp =
        NamedTempFile::new_in(path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "missing parent dir")
        })?)?;
    let serialized = toml::to_string_pretty(&state)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    temp.write_all(serialized.as_bytes())?;
    temp.flush()?;
    temp.persist(&path).map_err(|err| err.error)?;
    Ok(())
}

pub fn persist_model_selection(
    codex_home: &Path,
    workspace: &Path,
    model: &str,
    effort: Option<ReasoningEffort>,
) -> std::io::Result<()> {
    let mut state = load_workspace_state(codex_home, workspace)?;
    state.model = Some(model.to_string());
    state.model_reasoning_effort = effort;
    persist_workspace_state(codex_home, workspace, state)
}

pub fn persist_mcp_enabled(
    codex_home: &Path,
    workspace: &Path,
    server: &str,
    enabled: bool,
) -> std::io::Result<()> {
    let mut state = load_workspace_state(codex_home, workspace)?;
    state
        .mcp_servers
        .entry(server.to_string())
        .or_default()
        .enabled = Some(enabled);
    persist_workspace_state(codex_home, workspace, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn persists_and_loads_workspace_state() -> std::io::Result<()> {
        let codex_home = TempDir::new().expect("tempdir");
        let workspace = TempDir::new().expect("workspace");

        persist_model_selection(
            codex_home.path(),
            workspace.path(),
            "gpt-5-codex",
            Some(ReasoningEffort::High),
        )?;
        persist_mcp_enabled(codex_home.path(), workspace.path(), "docs", false)?;

        let state = load_workspace_state(codex_home.path(), workspace.path())?;
        assert_eq!(state.model.as_deref(), Some("gpt-5-codex"));
        assert_eq!(state.model_reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(
            state
                .mcp_servers
                .get("docs")
                .and_then(|entry| entry.enabled),
            Some(false)
        );
        Ok(())
    }
}
