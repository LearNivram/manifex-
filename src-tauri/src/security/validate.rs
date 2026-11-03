//! Input validation for every value that crosses the IPC boundary.
//!
//! The Kubernetes API server performs its own validation, but we validate early so
//! that malformed input never reaches URL construction, CLI arguments (Helm) or the
//! local database.

use crate::error::{AppError, AppResult};

/// RFC 1123 DNS subdomain (used for most object names). Max 253 chars.
pub fn dns_subdomain(field: &str, value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 253 {
        return Err(AppError::validation(format!(
            "{field} must be 1-253 characters"
        )));
    }
    let ok = value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    });
    if !ok {
        return Err(AppError::validation(format!(
            "{field} '{value}' is not a valid name (lowercase alphanumerics, '-' and '.')"
        )));
    }
    Ok(())
}

/// Object names. Some resources (e.g. RBAC roles, events) permit ':' and other
/// characters, so we accept a slightly broader, path-safe character set.
pub fn object_name(value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 253 {
        return Err(AppError::validation("name must be 1-253 characters"));
    }
    if value == "." || value == ".." {
        return Err(AppError::validation("invalid name"));
    }
    let ok = value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b':' | b'@'));
    if !ok {
        return Err(AppError::validation(format!(
            "name '{value}' contains invalid characters"
        )));
    }
    Ok(())
}

/// RFC 1123 DNS label (namespaces). Max 63 chars.
pub fn namespace(value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 63 {
        return Err(AppError::validation("namespace must be 1-63 characters"));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(AppError::validation(format!(
            "namespace '{value}' is invalid"
        )));
    }
    Ok(())
}

pub fn optional_namespace(value: &Option<String>) -> AppResult<()> {
    match value {
        Some(ns) if !ns.is_empty() => namespace(ns),
        _ => Ok(()),
    }
}

/// API group ("" for core) / version / plural.
pub fn api_segment(field: &str, value: &str, allow_empty: bool) -> AppResult<()> {
    if value.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(AppError::validation(format!("{field} is required")))
        };
    }
    if value.len() > 253
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return Err(AppError::validation(format!(
            "{field} '{value}' is invalid"
        )));
    }
    Ok(())
}

/// Label / field selectors are passed as query parameters; we only make sure they
/// don't contain control characters and are of reasonable length.
pub fn selector(value: &str) -> AppResult<()> {
    if value.len() > 2048 || value.chars().any(|c| c.is_control()) {
        return Err(AppError::validation("selector is invalid"));
    }
    Ok(())
}

/// Human readable display names (cluster names, groups, rule names).
pub fn display_name(field: &str, value: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 {
        return Err(AppError::validation(format!(
            "{field} must be 1-120 characters"
        )));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(AppError::validation(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(())
}

/// API server / agent endpoints must be HTTPS. Plain HTTP is never accepted because
/// credentials would be transmitted unencrypted.
pub fn https_url(field: &str, value: &str) -> AppResult<http::Uri> {
    let uri: http::Uri = value
        .trim()
        .parse()
        .map_err(|_| AppError::validation(format!("{field} is not a valid URL")))?;
    if uri.scheme_str() != Some("https") {
        return Err(AppError::Security(format!("{field} must use https://")));
    }
    if uri.host().is_none() {
        return Err(AppError::validation(format!("{field} must contain a host")));
    }
    Ok(uri)
}

/// Proxy URLs may be http(s) or socks5.
pub fn proxy_url(value: &str) -> AppResult<http::Uri> {
    let uri: http::Uri = value
        .trim()
        .parse()
        .map_err(|_| AppError::validation("proxy URL is invalid"))?;
    match uri.scheme_str() {
        Some("http") | Some("https") | Some("socks5") | Some("socks5h") => Ok(uri),
        _ => Err(AppError::validation(
            "proxy URL must use http, https or socks5",
        )),
    }
}

/// Container shell commands are restricted to an allow-list; we never execute
/// arbitrary user-supplied command lines inside containers implicitly.
pub fn shell(value: &str) -> AppResult<Vec<String>> {
    let cmd = match value {
        "auto" => vec![
            "/bin/sh".into(),
            "-c".into(),
            "TERM=xterm-256color; export TERM; \
             if command -v bash >/dev/null 2>&1; then exec bash; \
             elif command -v ash >/dev/null 2>&1; then exec ash; \
             else exec sh; fi"
                .into(),
        ],
        "bash" => vec!["/bin/bash".into()],
        "sh" => vec!["/bin/sh".into()],
        "ash" => vec!["/bin/ash".into()],
        "zsh" => vec!["/bin/zsh".into()],
        "powershell" => vec!["powershell.exe".into()],
        "cmd" => vec!["cmd.exe".into()],
        other => return Err(AppError::validation(format!("unsupported shell '{other}'"))),
    };
    Ok(cmd)
}

/// Helm release names follow DNS label rules with a 53 character limit.
pub fn helm_release(value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 53 {
        return Err(AppError::validation("release name must be 1-53 characters"));
    }
    namespace(value).map_err(|_| AppError::validation(format!("release name '{value}' is invalid")))
}

/// Helm chart references: `repo/chart`, `oci://…` or a chart name. They are passed
/// as discrete argv entries (never through a shell), but we still reject anything
/// that looks like a flag or contains whitespace.
pub fn helm_chart_ref(value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 512 || value.starts_with('-') {
        return Err(AppError::validation("chart reference is invalid"));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AppError::validation(
            "chart reference must not contain whitespace",
        ));
    }
    Ok(())
}

pub fn helm_version(value: &str) -> AppResult<()> {
    if value.len() > 64
        || value.starts_with('-')
        || !value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'.' | b'-' | b'+' | b'^' | b'~' | b'*' | b'<' | b'>' | b'=' | b' ' | b','
                )
        })
    {
        return Err(AppError::validation("chart version is invalid"));
    }
    Ok(())
}

pub fn helm_repo_name(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 100
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(AppError::validation("repository name is invalid"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        assert!(dns_subdomain("name", "my-app.v1").is_ok());
        assert!(dns_subdomain("name", "My-App").is_err());
        assert!(dns_subdomain("name", "-bad").is_err());
        assert!(namespace("kube-system").is_ok());
        assert!(namespace("kube_system").is_err());
        assert!(object_name("system:controller:foo").is_ok());
        assert!(object_name("../etc").is_err());
        assert!(object_name("a/b").is_err());
    }

    #[test]
    fn requires_https() {
        assert!(https_url("server", "https://10.0.0.1:6443").is_ok());
        assert!(https_url("server", "http://10.0.0.1:6443").is_err());
        assert!(https_url("server", "not a url").is_err());
    }

    #[test]
    fn shells_are_allow_listed() {
        assert!(shell("bash").is_ok());
        assert!(shell("rm -rf /").is_err());
    }

    #[test]
    fn helm_inputs() {
        assert!(helm_release("my-release").is_ok());
        assert!(helm_release("--set").is_err());
        assert!(helm_chart_ref("bitnami/nginx").is_ok());
        assert!(helm_chart_ref("oci://ghcr.io/x/y").is_ok());
        assert!(helm_chart_ref("--post-renderer").is_err());
        assert!(helm_chart_ref("a b").is_err());
        assert!(helm_version(">=1.2.0").is_ok());
        assert!(helm_version("--x").is_err());
    }
}
