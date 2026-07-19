pub mod app_state;
pub mod commands;
pub mod db;
#[cfg(all(feature = "e2e", debug_assertions))]
pub mod e2e;
pub mod error;
pub mod git;
pub mod identity;
pub mod jobs;
pub mod plugins;
pub mod secrets;
pub mod themes;

use tauri::Manager;

use plugins::protocol::{
    PLUGIN_PROTOCOL_SCHEME, build_plugin_response, service_unavailable_response,
};

macro_rules! invoke_handlers {
    ($($extra:path),* $(,)?) => {
        tauri::generate_handler![
            commands::get_app_info,
            commands::list_plugins,
            commands::list_themes,
            commands::current_theme,
            commands::activate_theme,
            commands::list_jobs,
            commands::authorize_plugin_call,
            commands::start_echo_job,
            commands::cancel_job,
            commands::git_project_create,
            commands::git_project_list,
            commands::git_project_update_scan_rules,
            commands::git_project_update,
            commands::git_project_delete,
            commands::git_project_scan,
            commands::git_workspace_create,
            commands::git_workspace_list,
            commands::git_workspace_get_membership,
            commands::git_workspace_update,
            commands::git_workspace_update_membership,
            commands::git_workspace_delete,
            commands::git_overview_get,
            commands::git_repository_snapshot,
            commands::git_repository_changes,
            commands::git_repository_diff,
            commands::git_repository_trust_status,
            commands::git_repository_stage,
            commands::git_repository_unstage,
            commands::git_repository_commit,
            commands::git_repository_trust,
            commands::git_identity_list,
            commands::git_identity_create,
            commands::git_identity_update,
            commands::git_identity_delete,
            commands::git_identity_set_global,
            commands::git_repository_bind_identity,
            commands::git_repository_unbind_identity,
            commands::git_repository_effective_identity
            $(, $extra)*
        ]
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol(PLUGIN_PROTOCOL_SCHEME, |context, request| {
            let Some(state) = context.app_handle().try_state::<app_state::AppState>() else {
                return service_unavailable_response();
            };
            build_plugin_response(&state.plugins, &request)
        });
    // The WebDriver server is intentionally unavailable in release builds,
    // even if a downstream build accidentally passes the e2e feature.
    #[cfg(all(feature = "e2e", debug_assertions))]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    let builder = builder.setup(|app| {
        let state = app_state::AppState::bootstrap(app.handle())?;
        app.manage(state);
        Ok(())
    });
    #[cfg(all(feature = "e2e", debug_assertions))]
    let builder = builder.invoke_handler(invoke_handlers![
        e2e::e2e_seed_fixture,
        e2e::e2e_app_data_paths
    ]);
    #[cfg(not(all(feature = "e2e", debug_assertions)))]
    let builder = builder.invoke_handler(invoke_handlers![]);
    builder
        .run(tauri::generate_context!())
        .expect("Git-Ramus failed to start");
}

#[cfg(test)]
const fn e2e_seed_fixture_handler_enabled() -> bool {
    cfg!(all(feature = "e2e", debug_assertions))
}

#[cfg(test)]
mod tests {
    #[test]
    fn e2e_seed_fixture_handler_matches_the_debug_feature_boundary() {
        assert_eq!(
            super::e2e_seed_fixture_handler_enabled(),
            cfg!(all(feature = "e2e", debug_assertions))
        );
    }
}
