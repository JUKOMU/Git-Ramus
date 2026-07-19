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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeContribution {
    pub theme_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_path: Option<String>,
}

impl ThemeContribution {
    pub fn definition_path(&self) -> Result<&str, crate::error::AppError> {
        match (self.definition.as_deref(), self.definition_path.as_deref()) {
            (Some(definition), Some(definition_path)) if definition != definition_path => {
                Err(crate::error::AppError::InvalidInput(
                    "theme definition and definitionPath must match".to_owned(),
                ))
            }
            (Some(definition), _) => Ok(definition),
            (None, Some(definition_path)) => Ok(definition_path),
            (None, None) => Err(crate::error::AppError::InvalidInput(
                "theme definition path is required".to_owned(),
            )),
        }
    }
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
        if !is_safe_relative_path(&self.entrypoints.ui) {
            return Err(crate::error::AppError::InvalidInput(
                "plugin UI entrypoint escapes its root".to_owned(),
            ));
        }
        if let Some(theme) = &self.contributions.theme {
            if !is_plugin_id(&theme.theme_id) {
                return Err(crate::error::AppError::InvalidInput(
                    "theme id is invalid".to_owned(),
                ));
            }
            let definition_path = theme.definition_path()?;
            if !is_safe_relative_path(definition_path) {
                return Err(crate::error::AppError::InvalidInput(
                    "theme definition path escapes its plugin root".to_owned(),
                ));
            }
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

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !std::path::Path::new(value).is_absolute()
        && !value.starts_with(['/', '\\'])
        && !has_url_scheme(value)
        && !value.chars().any(char::is_control)
        && !value.split(['/', '\\']).any(|component| component == "..")
}

fn has_url_scheme(value: &str) -> bool {
    let Some((prefix, _)) = value.split_once(':') else {
        return false;
    };
    let mut characters = prefix.chars();
    matches!(characters.next(), Some(character) if character.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric()
                || character == '+'
                || character == '-'
                || character == '.'
        })
}

#[cfg(test)]
mod tests {
    use super::{PluginManifest, ThemeContribution};

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

    #[test]
    fn manifest_accepts_a_safe_theme_contribution() {
        let manifest: PluginManifest = serde_json::from_str(
            r#"{"schemaVersion":1,"id":"git-ramus.compact-theme","name":"Compact","version":"0.1.0","publisher":"git-ramus","description":"Compact theme","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[],"theme":{"themeId":"git-ramus.theme.compact","definition":"theme.json"}},"permissions":[]}"#,
        )
        .expect("safe theme contribution parses");

        manifest
            .validate()
            .expect("safe theme contribution validates");
    }

    #[test]
    fn manifest_rejects_unsafe_theme_definition_paths() {
        let mut manifest: PluginManifest = serde_json::from_str(
            r#"{"schemaVersion":1,"id":"git-ramus.compact-theme","name":"Compact","version":"0.1.0","publisher":"git-ramus","description":"Compact theme","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[]},"permissions":[]}"#,
        )
        .expect("manifest parses");

        for path in [
            "../theme.json",
            r"..\theme.json",
            r"C:\theme.json",
            "C:theme.json",
            "/theme.json",
            r"\theme.json",
            "https://evil.test/theme.json",
            "data:text/html,evil",
            "file:theme.json",
            "theme.json\0",
            "theme\n.json",
        ] {
            manifest.contributions.theme = Some(ThemeContribution {
                theme_id: "git-ramus.theme.compact".to_owned(),
                definition: Some(path.to_owned()),
                definition_path: None,
            });
            assert!(
                manifest.validate().is_err(),
                "unsafe theme definition path was accepted: {path:?}"
            );
        }
    }

    #[test]
    fn manifest_supports_definition_path_alias_without_ambiguity() {
        let mut manifest: PluginManifest = serde_json::from_str(
            r#"{"schemaVersion":1,"id":"git-ramus.compact-theme","name":"Compact","version":"0.1.0","publisher":"git-ramus","description":"Compact theme","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[]},"permissions":[]}"#,
        )
        .expect("manifest parses");
        manifest.contributions.theme = Some(ThemeContribution {
            theme_id: "git-ramus.theme.compact".to_owned(),
            definition: None,
            definition_path: Some("theme.json".to_owned()),
        });
        manifest
            .validate()
            .expect("definitionPath compatibility alias validates");

        manifest.contributions.theme = Some(ThemeContribution {
            theme_id: "git-ramus.theme.compact".to_owned(),
            definition: None,
            definition_path: None,
        });
        assert!(manifest.validate().is_err());

        manifest.contributions.theme = Some(ThemeContribution {
            theme_id: "bad".to_owned(),
            definition: Some("theme.json".to_owned()),
            definition_path: None,
        });
        assert!(manifest.validate().is_err());

        manifest.contributions.theme = Some(ThemeContribution {
            theme_id: "git-ramus.theme.compact".to_owned(),
            definition: Some("theme.json".to_owned()),
            definition_path: Some("other.json".to_owned()),
        });
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_rejects_scheme_and_control_character_ui_entrypoints() {
        let mut manifest: PluginManifest =
            serde_json::from_str(WELCOME_MANIFEST).expect("manifest parses");
        for entrypoint in [
            "https://evil.test/ui.html",
            "data:text/html,evil",
            "file:ui.html",
            "ui.html\0",
            "ui\n.html",
        ] {
            manifest.entrypoints.ui = entrypoint.to_owned();
            assert!(
                manifest.validate().is_err(),
                "unsafe UI entrypoint was accepted: {entrypoint:?}"
            );
        }
    }

    #[test]
    fn optional_theme_fields_are_omitted_for_the_typescript_contract() {
        let welcome: PluginManifest =
            serde_json::from_str(WELCOME_MANIFEST).expect("welcome manifest parses");
        let welcome_json = serde_json::to_value(welcome).expect("welcome serializes");
        assert!(welcome_json["contributions"].get("theme").is_none());

        let compact: PluginManifest = serde_json::from_str(
            r#"{"schemaVersion":1,"id":"git-ramus.compact-theme","name":"Compact","version":"0.1.0","publisher":"git-ramus","description":"Compact theme","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[],"theme":{"themeId":"git-ramus.theme.compact","definition":"theme.json"}},"permissions":[]}"#,
        )
        .expect("compact manifest parses");
        let compact_json = serde_json::to_value(compact).expect("compact serializes");
        let theme = &compact_json["contributions"]["theme"];
        assert_eq!(theme["definition"], "theme.json");
        assert!(theme.get("definitionPath").is_none());
    }
}
