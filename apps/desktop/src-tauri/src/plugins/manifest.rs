use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub description: String,
    pub kind: PluginKind,
    pub sdk_version: String,
    pub entrypoints: PluginEntrypoints,
    pub contributions: PluginContributions,
    pub permissions: Vec<PermissionRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Builtin,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEntrypoints {
    pub ui: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginContributions {
    pub navigation: Vec<NavigationContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationContribution {
    pub id: String,
    pub label: String,
    pub route: String,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub capability: String,
    pub resources: Vec<String>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.schema_version != 1 {
            return Err(crate::error::AppError::InvalidInput(
                "plugin schemaVersion must be 1".to_owned(),
            ));
        }
        if !is_plugin_id(&self.id) {
            return Err(crate::error::AppError::InvalidInput(
                "plugin id is invalid".to_owned(),
            ));
        }
        semver::Version::parse(&self.version).map_err(|error| {
            crate::error::AppError::InvalidInput(format!("plugin version is invalid: {error}"))
        })?;
        semver::VersionReq::parse(&self.sdk_version).map_err(|error| {
            crate::error::AppError::InvalidInput(format!("sdkVersion is invalid: {error}"))
        })?;
        let entrypoint_text = self.entrypoints.ui.as_str();
        let bytes = entrypoint_text.as_bytes();
        let has_windows_drive_prefix =
            bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        let entrypoint = std::path::Path::new(entrypoint_text);
        if entrypoint_text.is_empty()
            || entrypoint.is_absolute()
            || entrypoint_text.starts_with('/')
            || entrypoint_text.starts_with('\\')
            || has_windows_drive_prefix
            || entrypoint_text
                .split(['/', '\\'])
                .any(|component| component == "..")
        {
            return Err(crate::error::AppError::InvalidInput(
                "plugin UI entrypoint escapes its root".to_owned(),
            ));
        }
        for permission in &self.permissions {
            if !is_capability(&permission.capability)
                || permission.resources.is_empty()
                || permission.resources.iter().any(String::is_empty)
            {
                return Err(crate::error::AppError::InvalidInput(
                    "plugin permission is invalid".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

fn is_plugin_id(value: &str) -> bool {
    let mut contains_dot = false;
    let mut previous_was_separator = true;
    for character in value.chars() {
        match character {
            '.' | '-' => {
                if previous_was_separator {
                    return false;
                }
                contains_dot |= character == '.';
                previous_was_separator = true;
            }
            'a'..='z' | '0'..='9' => previous_was_separator = false,
            _ => return false,
        }
    }
    contains_dot && !previous_was_separator
}

fn is_capability(value: &str) -> bool {
    let mut parts = value.split(':');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(namespace), Some(action), None)
            if is_capability_part(namespace) && is_capability_part(action)
    )
}

fn is_capability_part(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(character) if character.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '.'
                || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use super::PluginManifest;

    const WELCOME_MANIFEST: &str =
        include_str!("../../../../../plugins/builtin-welcome/plugin.json");

    #[test]
    fn rust_contract_reads_the_same_welcome_manifest_as_typescript() {
        let manifest: PluginManifest =
            serde_json::from_str(WELCOME_MANIFEST).expect("manifest parses");
        manifest.validate().expect("manifest validates");
        assert_eq!(manifest.id, "git-ramus.welcome");
        assert_eq!(manifest.entrypoints.ui, "ui.html");
    }

    #[test]
    fn manifest_rejects_unsafe_entrypoints_on_every_platform() {
        let mut manifest: PluginManifest =
            serde_json::from_str(WELCOME_MANIFEST).expect("manifest parses");
        for entrypoint in [
            "../secret.html",
            r"..\secret.html",
            r"C:\secret.html",
            "C:secret.html",
        ] {
            manifest.entrypoints.ui = entrypoint.to_owned();
            assert!(manifest.validate().is_err());
        }
    }
}
