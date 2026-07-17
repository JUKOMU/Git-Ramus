use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::error::{AppError, ErrorEnvelope};
use crate::jobs::model::Job;
use crate::plugins::PluginDescriptor;

type CommandResult<T> = Result<T, Box<ErrorEnvelope>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizationRequest {
    pub plugin_id: String,
    pub capability: String,
    pub resource: String,
}

#[derive(Debug, Serialize)]
pub struct AuthorizationDecision {
    pub allowed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EchoJobRequest {
    pub plugin_id: String,
    pub message: String,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    app_info()
}

pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Git-Ramus".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

#[tauri::command]
pub fn list_plugins(state: State<'_, AppState>) -> Vec<PluginDescriptor> {
    state.plugins.descriptors().to_vec()
}

#[tauri::command]
pub fn list_jobs(state: State<'_, AppState>) -> CommandResult<Vec<Job>> {
    state.jobs.list().map_err(command_error)
}

#[tauri::command]
pub fn authorize_plugin_call(
    state: State<'_, AppState>,
    request: AuthorizationRequest,
) -> CommandResult<AuthorizationDecision> {
    state
        .permissions
        .is_allowed(&request.plugin_id, &request.capability, &request.resource)
        .map(|allowed| AuthorizationDecision { allowed })
        .map_err(command_error)
}

#[tauri::command]
pub async fn start_echo_job(
    app: AppHandle,
    state: State<'_, AppState>,
    request: EchoJobRequest,
) -> CommandResult<Job> {
    if request.message.trim().is_empty() || request.message.chars().count() > 256 {
        return Err(command_error(AppError::InvalidInput(
            "echo message must contain 1 to 256 characters".to_owned(),
        )));
    }
    if !state
        .permissions
        .is_allowed(&request.plugin_id, "tasks:create", "echo")
        .map_err(command_error)?
    {
        return Err(command_error(AppError::PermissionDenied));
    }
    let job = state
        .jobs
        .create("system.echo", &format!("Echo {}", request.message))
        .map_err(command_error)?;
    let runner = state.jobs.clone();
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        if let Ok(started) = runner.start(&job_id) {
            let _ = app.emit("job://updated", started);
        }
        for progress in [0.25, 0.5, 0.75] {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if runner.is_canceled(&job_id).unwrap_or(true) {
                return;
            }
            if let Ok(updated) = runner.set_progress(&job_id, progress) {
                let _ = app.emit("job://updated", updated);
            }
        }
        if let Ok(completed) = runner.succeed(&job_id) {
            let _ = app.emit("job://updated", completed);
        }
    });
    Ok(job)
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, state: State<'_, AppState>, job_id: String) -> CommandResult<()> {
    let canceled = state.jobs.cancel(&job_id).map_err(command_error)?;
    let _ = app.emit("job://updated", canceled);
    Ok(())
}

fn command_error(error: AppError) -> Box<ErrorEnvelope> {
    Box::new(ErrorEnvelope::from(error))
}

#[cfg(test)]
mod tests {
    use super::app_info;

    #[test]
    fn app_info_uses_compile_time_package_metadata() {
        let info = app_info();
        assert_eq!(info.name, "Git-Ramus");
        assert_eq!(info.version, "0.1.0");
    }
}
