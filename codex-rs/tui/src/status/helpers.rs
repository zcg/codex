use crate::exec_command::relativize_to_home;
use crate::text_formatting;
use chrono::DateTime;
use chrono::Local;
use codex_core::auth::get_auth_file;
use codex_core::auth::try_read_auth_json;
use codex_core::config::Config;
use codex_core::git_info::get_git_repo_root;
use codex_core::project_doc::discover_project_doc_paths;
use std::path::Path;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::account::StatusAccountDisplay;

fn normalize_agents_display_path(path: &Path) -> String {
    dunce::simplified(path).display().to_string()
}

pub(crate) fn compose_model_display(
    config: &Config,
    entries: &[(&str, String)],
) -> (String, Vec<String>) {
    let mut details: Vec<String> = Vec::new();
    if let Some((_, effort)) = entries.iter().find(|(k, _)| *k == "reasoning effort") {
        details.push(format!("reasoning {}", effort.to_ascii_lowercase()));
    }
    if let Some((_, summary)) = entries.iter().find(|(k, _)| *k == "reasoning summaries") {
        let summary = summary.trim();
        if summary.eq_ignore_ascii_case("none") || summary.eq_ignore_ascii_case("off") {
            details.push("summaries off".to_string());
        } else if !summary.is_empty() {
            details.push(format!("summaries {}", summary.to_ascii_lowercase()));
        }
    }

    (config.model.clone(), details)
}

pub(crate) fn compose_agents_summary(config: &Config) -> String {
    match discover_project_doc_paths(config) {
        Ok(paths) => {
            let mut rels: Vec<String> = Vec::new();
            for p in paths {
                let file_name = p
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                let display = if let Some(parent) = p.parent() {
                    if parent == config.cwd {
                        file_name.clone()
                    } else {
                        let mut cur = config.cwd.as_path();
                        let mut ups = 0usize;
                        let mut reached = false;
                        while let Some(c) = cur.parent() {
                            if cur == parent {
                                reached = true;
                                break;
                            }
                            cur = c;
                            ups += 1;
                        }
                        if reached {
                            let up = format!("..{}", std::path::MAIN_SEPARATOR);
                            format!("{}{}", up.repeat(ups), file_name)
                        } else if let Ok(stripped) = p.strip_prefix(&config.cwd) {
                            normalize_agents_display_path(stripped)
                        } else {
                            normalize_agents_display_path(&p)
                        }
                    }
                } else {
                    normalize_agents_display_path(&p)
                };
                rels.push(display);
            }
            if rels.is_empty() {
                "<none>".to_string()
            } else {
                rels.join(", ")
            }
        }
        Err(_) => "<none>".to_string(),
    }
}

pub(crate) fn compose_account_display(config: &Config) -> Option<StatusAccountDisplay> {
    let auth_file = get_auth_file(&config.codex_home);
    let auth = try_read_auth_json(&auth_file).ok()?;

    if let Some(tokens) = auth.tokens.as_ref() {
        let info = &tokens.id_token;
        let email = info.email.clone();
        let plan = info.get_chatgpt_plan_type().map(|plan| title_case(&plan));
        return Some(StatusAccountDisplay::ChatGpt { email, plan });
    }

    if let Some(key) = auth.openai_api_key
        && !key.is_empty()
    {
        return Some(StatusAccountDisplay::ApiKey);
    }

    None
}

pub(crate) fn format_tokens_compact(value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    if value < 1_000 {
        return value.to_string();
    }

    let (scaled, suffix) = if value >= 1_000_000_000_000 {
        (value as f64 / 1_000_000_000_000.0, "T")
    } else if value >= 1_000_000_000 {
        (value as f64 / 1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "M")
    } else {
        (value as f64 / 1_000.0, "K")
    };

    let decimals = if scaled < 10.0 {
        2
    } else if scaled < 100.0 {
        1
    } else {
        0
    };

    let mut formatted = format!("{scaled:.decimals$}");
    if formatted.contains('.') {
        while formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
    }

    format!("{formatted}{suffix}")
}

pub(crate) fn format_directory_display(directory: &Path, max_width: Option<usize>) -> String {
    const TRUNCATION_LENGTH: usize = 2;
    const FISH_STYLE_LEN: usize = 2;
    const TRUNCATION_SYMBOL: &str = "…";

    let simplified = dunce::simplified(directory);

    let (mut display_segments, rel_segments) = match relativize_to_home(simplified) {
        Some(rel) => {
            let segments = path_segments(&rel);
            (vec!["~".to_string()], segments)
        }
        None => {
            let mut prefix = String::new();
            let mut segments: Vec<String> = Vec::new();
            for component in simplified.components() {
                use std::path::Component;
                match component {
                    Component::Prefix(p) => {
                        prefix = p.as_os_str().to_string_lossy().into_owned();
                    }
                    Component::RootDir => {
                        if prefix.is_empty() {
                            prefix = std::path::MAIN_SEPARATOR.to_string();
                        }
                    }
                    Component::Normal(os) => {
                        segments.push(os.to_string_lossy().into_owned());
                    }
                    Component::CurDir => {}
                    Component::ParentDir => segments.push("..".to_string()),
                }
            }
            let mut initial = Vec::new();
            if !prefix.is_empty() {
                // Represent root directories as empty so join inserts separator.
                if prefix == std::path::MAIN_SEPARATOR.to_string() {
                    initial.push(String::new());
                } else {
                    initial.push(prefix);
                }
            }
            (initial, segments)
        }
    };

    let repo_root = get_git_repo_root(simplified);
    let repo_segments = repo_root.as_ref().and_then(|root| {
        relativize_to_home(root)
            .map(|rel| path_segments(&rel))
            .or_else(|| Some(path_segments(dunce::simplified(root))))
    });

    let repo_index = repo_segments.as_ref().and_then(|segments| {
        if segments.is_empty() {
            return None;
        }
        if segments.len() <= rel_segments.len() && rel_segments[..segments.len()] == *segments {
            Some(segments.len() - 1)
        } else {
            None
        }
    });

    let (prefix_count, mut tail_segments) = if let Some(idx) = repo_index {
        (idx, rel_segments[idx..].to_vec())
    } else {
        let tail_start = rel_segments.len().saturating_sub(TRUNCATION_LENGTH);
        (tail_start, rel_segments[tail_start..].to_vec())
    };

    let prefix_segments = rel_segments[..prefix_count].to_vec();
    let truncated_prefix: Vec<String> = prefix_segments
        .into_iter()
        .map(|segment| truncate_segment(&segment, FISH_STYLE_LEN))
        .collect();

    let mut truncated_tail = false;
    if tail_segments.len() > TRUNCATION_LENGTH {
        truncated_tail = true;
        let mut kept: Vec<String> =
            tail_segments[tail_segments.len() - TRUNCATION_LENGTH..].to_vec();
        if let Some(root) = tail_segments.first().cloned()
            && !kept.iter().any(|seg| seg == &root)
        {
            kept.insert(0, root);
        }
        tail_segments = kept;
    }

    display_segments.extend(truncated_prefix);
    if truncated_tail && !tail_segments.is_empty() {
        display_segments.push(TRUNCATION_SYMBOL.to_string());
    }
    display_segments.extend(tail_segments);

    let formatted = if display_segments.is_empty() {
        if let Some(repo_root) = repo_root {
            repo_root.display().to_string()
        } else {
            simplified.display().to_string()
        }
    } else {
        let sep = std::path::MAIN_SEPARATOR.to_string();
        display_segments.join(sep.as_str())
    };

    if let Some(max_width) = max_width {
        if max_width == 0 {
            return String::new();
        }
        if UnicodeWidthStr::width(formatted.as_str()) > max_width {
            return text_formatting::center_truncate_path(&formatted, max_width);
        }
    }

    formatted
}

pub(crate) fn format_reset_timestamp(dt: DateTime<Local>, captured_at: DateTime<Local>) -> String {
    let time = dt.format("%H:%M").to_string();
    if dt.date_naive() == captured_at.date_naive() {
        time
    } else {
        format!("{time} on {}", dt.format("%-d %b"))
    }
}

pub(crate) fn title_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut chars = s.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return String::new(),
    };
    let rest: String = chars.as_str().to_ascii_lowercase();
    first.to_uppercase().collect::<String>() + &rest
}

fn path_segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(os) => Some(os.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => Some("..".to_string()),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .collect()
}

fn truncate_segment(segment: &str, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let mut result = String::new();
    for grapheme in segment.graphemes(true).take(len) {
        result.push_str(grapheme);
    }
    if result.is_empty() {
        segment
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default()
    } else {
        result
    }
}
