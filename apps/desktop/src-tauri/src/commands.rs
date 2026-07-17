use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
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
