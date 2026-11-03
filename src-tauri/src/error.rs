//! Unified error type for the native core.
//!
//! Errors are serialised to the frontend as `{ code, message, status? }` so the UI
//! can render meaningful messages without ever receiving internal details such as
//! credentials or file contents.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("cluster '{0}' is not connected")]
    NotConnected(String),
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("security policy violation: {0}")]
    Security(String),
    #[error("secure credential store unavailable: {0}")]
    Vault(String),
    #[error("{message}")]
    Kube { message: String, status: Option<u16>, reason: Option<String> },
    #[error("database error: {0}")]
    Database(String),
    #[error("i/o error: {0}")]
    Io(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("{0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Validation(_) => "validation",
            AppError::NotFound(_) => "not_found",
            AppError::NotConnected(_) => "not_connected",
            AppError::Connection(_) => "connection",
            AppError::Security(_) => "security",
            AppError::Vault(_) => "vault",
            AppError::Kube { status: Some(403), .. } => "forbidden",
            AppError::Kube { status: Some(404), .. } => "not_found",
            AppError::Kube { status: Some(409), .. } => "conflict",
            AppError::Kube { .. } => "kube",
            AppError::Database(_) => "database",
            AppError::Io(_) => "io",
            AppError::Cancelled => "cancelled",
            AppError::Internal(_) => "internal",
        }
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }
}

#[derive(Serialize)]
struct SerializedError<'a> {
    code: &'a str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (status, reason) = match self {
            AppError::Kube { status, reason, .. } => (*status, reason.as_deref()),
            _ => (None, None),
        };
        SerializedError { code: self.code(), message: self.to_string(), status, reason }
            .serialize(serializer)
    }
}

impl From<kube::Error> for AppError {
    fn from(err: kube::Error) -> Self {
        match err {
            kube::Error::Api(status) => AppError::Kube {
                message: if status.message.is_empty() {
                    status.reason.clone()
                } else {
                    status.message.clone()
                },
                status: Some(status.code),
                reason: Some(status.reason.clone()),
            },
            kube::Error::Auth(e) => AppError::Connection(format!("authentication failed: {e}")),
            kube::Error::HyperError(e) => AppError::Connection(e.to_string()),
            kube::Error::Service(e) => AppError::Connection(e.to_string()),
            kube::Error::RustlsTls(e) => AppError::Connection(format!("TLS error: {e}")),
            other => AppError::Kube { message: other.to_string(), status: None, reason: None },
        }
    }
}

impl From<kube::config::KubeconfigError> for AppError {
    fn from(err: kube::config::KubeconfigError) -> Self {
        AppError::Validation(format!("invalid kubeconfig: {err}"))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("record".into()),
            other => AppError::Database(other.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Validation(format!("invalid JSON: {err}"))
    }
}

impl From<serde_yaml::Error> for AppError {
    fn from(err: serde_yaml::Error) -> Self {
        let loc = err.location().map(|l| format!(" (line {}, column {})", l.line(), l.column()));
        AppError::Validation(format!("invalid YAML{}: {err}", loc.unwrap_or_default()))
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(err: tokio::task::JoinError) -> Self {
        AppError::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_code() {
        let e = AppError::Kube { message: "denied".into(), status: Some(403), reason: Some("Forbidden".into()) };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "forbidden");
        assert_eq!(v["status"], 403);
        assert_eq!(v["message"], "denied");
    }
}
