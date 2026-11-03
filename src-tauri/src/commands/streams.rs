//! Streaming commands: container logs and interactive terminals.

use tauri::ipc::{Channel, Response};
use tauri::State;

use crate::error::AppResult;
use crate::k8s::exec::{ExecEvent, ExecTarget};
use crate::k8s::logs::{LogMessage, LogOptions};
use crate::state::{AppState, AuditTarget};

#[tauri::command]
pub async fn logs_start(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    pod: String,
    options: LogOptions,
    on_event: Channel<LogMessage>,
) -> AppResult<String> {
    let conn = state.conn(&cluster_id)?;
    state.logs.start(conn.client.clone(), &namespace, &pod, options, move |m| on_event.send(m).is_ok()).await
}

#[tauri::command]
pub async fn logs_stop(state: State<'_, AppState>, stream_id: String) -> AppResult<()> {
    state.logs.stop(&stream_id);
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn exec_start(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    pod: String,
    container: Option<String>,
    shell: String,
    cols: u16,
    rows: u16,
    on_data: Channel<Response>,
    on_event: Channel<ExecEvent>,
) -> AppResult<String> {
    let conn = state.conn(&cluster_id)?;
    let exec = state.exec.clone();
    let target = ExecTarget {
        cluster_id: &cluster_id,
        namespace: &namespace,
        pod: &pod,
        container: container.as_deref(),
        shell: &shell,
        cols,
        rows,
    };
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "exec",
                resource: Some("Pod".into()),
                namespace: Some(namespace.clone()),
                name: Some(pod.clone()),
                detail: Some(format!("container={} shell={shell}", container.clone().unwrap_or_default())),
            },
            exec.start(
                conn.client.clone(),
                target,
                move |bytes| on_data.send(Response::new(bytes)).is_ok(),
                move |ev| {
                    let _ = on_event.send(ev);
                },
            ),
        )
        .await
}

#[tauri::command]
pub async fn exec_write(state: State<'_, AppState>, session_id: String, data: String) -> AppResult<()> {
    state.exec.write(&session_id, data.into_bytes()).await
}

#[tauri::command]
pub async fn exec_resize(state: State<'_, AppState>, session_id: String, cols: u16, rows: u16) -> AppResult<()> {
    state.exec.resize(&session_id, cols, rows).await
}

#[tauri::command]
pub async fn exec_close(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    state.exec.close(&session_id);
    Ok(())
}
