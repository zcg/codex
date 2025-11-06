use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let sha = env_sha()
        .or_else(git_sha)
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=CODEX_CLI_GIT_SHA={sha}");
}

fn env_sha() -> Option<String> {
    let value = env::var("CODEX_BUILD_GIT_SHA").ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn git_sha() -> Option<String> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").ok()?;
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        return None;
    }
    Some(sha)
}
