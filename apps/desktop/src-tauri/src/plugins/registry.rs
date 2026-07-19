use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::AppError;
use crate::plugins::manifest::PluginManifest;
use crate::plugins::protocol::plugin_ui_url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub manifest: PluginManifest,
    pub ui_url: String,
    #[serde(skip_serializing)]
    pub ui_html: String,
    #[serde(skip_serializing)]
    pub(crate) root_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    descriptors: Vec<PluginDescriptor>,
}

impl PluginRegistry {
    pub fn discover(root: &Path) -> Result<Self, AppError> {
        Self::discover_many(&[root])
    }

    pub fn discover_many(roots: &[&Path]) -> Result<Self, AppError> {
        let mut descriptors = Vec::new();
        for root in roots {
            if !root.exists() {
                continue;
            }
            let mut directories = Vec::new();
            for entry in std::fs::read_dir(root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    directories.push(entry.path());
                }
            }
            directories.sort();
            for directory in directories {
                let manifest_text = std::fs::read_to_string(directory.join("plugin.json"))?;
                let manifest: PluginManifest = serde_json::from_str(&manifest_text)?;
                manifest.validate()?;
                let directory_id = directory
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .ok_or_else(|| {
                        AppError::InvalidInput("plugin directory is not UTF-8".to_owned())
                    })?;
                if directory_id != manifest.id.as_str() {
                    return Err(AppError::InvalidInput(format!(
                        "plugin directory {directory_id} does not match manifest id {}",
                        manifest.id
                    )));
                }
                if descriptors
                    .iter()
                    .any(|descriptor: &PluginDescriptor| descriptor.manifest.id == manifest.id)
                {
                    return Err(AppError::InvalidInput(format!(
                        "duplicate plugin id: {}",
                        manifest.id
                    )));
                }
                let canonical_root = directory.canonicalize()?;
                let entrypoint = directory.join(&manifest.entrypoints.ui).canonicalize()?;
                if !entrypoint.starts_with(&canonical_root) {
                    return Err(AppError::InvalidInput(
                        "plugin entrypoint escapes its root".to_owned(),
                    ));
                }
                let ui_html = std::fs::read_to_string(entrypoint)?;
                let ui_url = plugin_ui_url(&manifest.id);
                descriptors.push(PluginDescriptor {
                    manifest,
                    ui_url,
                    ui_html,
                    root_path: canonical_root,
                });
            }
        }
        descriptors.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        Ok(Self { descriptors })
    }

    pub fn descriptors(&self) -> &[PluginDescriptor] {
        &self.descriptors
    }

    pub fn get(&self, plugin_id: &str) -> Option<&PluginDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.manifest.id == plugin_id)
    }
}

impl PluginDescriptor {
    pub(crate) fn root_path(&self) -> &Path {
        &self.root_path
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::PluginRegistry;

    fn write_plugin(root: &std::path::Path, id: &str, html: &str) {
        let directory = root.join(id);
        fs::create_dir_all(&directory).expect("plugin directory creates");
        let manifest = format!(
            r#"{{"schemaVersion":1,"id":"{id}","name":"Test","version":"0.1.0","publisher":"git-ramus","description":"Test plugin","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{{"ui":"ui.html"}},"contributions":{{"navigation":[]}},"permissions":[{{"capability":"app:read","resources":["info"]}}]}}"#
        );
        fs::write(directory.join("plugin.json"), manifest).expect("manifest writes");
        fs::write(directory.join("ui.html"), html).expect("UI writes");
    }

    #[test]
    fn discovers_plugins_in_stable_id_order() {
        let directory = tempdir().expect("temp directory creates");
        write_plugin(directory.path(), "git-ramus.zeta", "<h1>Zeta</h1>");
        write_plugin(directory.path(), "git-ramus.alpha", "<h1>Alpha</h1>");
        let registry = PluginRegistry::discover(directory.path()).expect("discovery succeeds");
        let ids = registry
            .descriptors()
            .iter()
            .map(|descriptor| descriptor.manifest.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["git-ramus.alpha", "git-ramus.zeta"]);
    }

    #[test]
    fn duplicate_plugin_ids_are_rejected() {
        let first = tempdir().expect("temp directory creates");
        let second = tempdir().expect("temp directory creates");
        write_plugin(first.path(), "git-ramus.same", "<h1>First</h1>");
        write_plugin(second.path(), "git-ramus.same", "<h1>Second</h1>");
        assert!(PluginRegistry::discover_many(&[first.path(), second.path()]).is_err());
    }
}
