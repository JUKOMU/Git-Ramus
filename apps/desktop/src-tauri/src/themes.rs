use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::error::AppError;
use crate::plugins::manifest::{ThemeContribution, is_safe_text};
use crate::plugins::{PluginDescriptor, PluginRegistry};

pub const DEFAULT_THEME_ID: &str = "git-ramus.theme.default";
const HOST_THEME_PLUGIN_ID: &str = "git-ramus.host";
const MAX_THEME_BYTES: u64 = 64 * 1024;
const UNNAMED_THEME_NAME: &str = "Unnamed theme";
const STALE_THEME_REASON: &str = "theme.plugin.stale";
const DEFAULT_THEME_JSON: &str = r##"{
  "themeId":"git-ramus.theme.default",
  "name":"Git-Ramus Default",
  "colors":{
    "background":"#080d18","surface":"#111827","surfaceRaised":"#1e293b",
    "text":"#e2e8f0","textMuted":"#64748b","border":"#263247",
    "primary":"#0ea5e9","secondary":"#38bdf8","accent":"#22d3ee",
    "success":"#4ade80","warning":"#fbbf24","danger":"#fb7185","focusRing":"#7dd3fc"
  },
  "typography":{"fontFamily":"Inter, system-ui, sans-serif","fontSize":16,"lineHeight":1.5,"fontWeight":400,"letterSpacing":0},
  "spacing":{"unit":4,"xs":4,"sm":8,"md":12,"lg":20,"xl":28},
  "shape":{"radius":8,"radiusSm":6,"radiusMd":8,"radiusLg":12},
  "elevation":{"none":"none","sm":"0 1px 2px #0003","md":"0 8px 24px #0004","lg":"0 16px 40px #0005"},
  "motion":{"durationFast":"120ms","durationNormal":"180ms","durationSlow":"260ms","easing":"ease-out"},
  "density":"comfortable"
}"##;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeDefinition {
    pub theme_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<ThemeColors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typography: Option<ThemeTypography>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing: Option<ThemeSpacing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<ThemeShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation: Option<ThemeElevation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<ThemeMotion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<ThemeDensity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeColors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_raised: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_ring: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeTypography {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<ThemeFontWeight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<ThemeLength>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeSpacing {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xs: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sm: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lg: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xl: Option<ThemeLength>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeShape {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_sm: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_md: Option<ThemeLength>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_lg: Option<ThemeLength>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeElevation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level2: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level3: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeMotion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_fast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_normal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_slow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub easing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeLength {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeFontWeight {
    Number(u16),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeDensity {
    Comfortable,
    Compact,
}

impl ThemeDensity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Compact => "compact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeMetadata {
    pub theme_id: String,
    pub name: String,
    pub plugin_id: String,
    pub version: String,
    pub density: ThemeDensity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeState {
    pub active_theme_id: String,
    pub theme: ThemeDefinition,
}

#[derive(Debug, Clone)]
struct ThemeRecord {
    metadata: ThemeMetadata,
    definition: ThemeDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeLoadError {
    ReadFailed,
    InvalidJson,
    InvalidSchema,
    IdMismatch,
}

impl ThemeLoadError {
    const fn reason_code(self) -> &'static str {
        match self {
            Self::ReadFailed => "theme.definition.read-failed",
            Self::InvalidJson => "theme.definition.invalid-json",
            Self::InvalidSchema => "theme.definition.invalid-schema",
            Self::IdMismatch => "theme.definition.id-mismatch",
        }
    }
}

#[derive(Clone)]
pub struct ThemeManager {
    database: Database,
    themes: Arc<Vec<ThemeRecord>>,
}

impl ThemeManager {
    pub fn discover(database: Database, plugins: &PluginRegistry) -> Result<Self, AppError> {
        invalidate_plugin_themes(&database)?;
        let default_definition = parse_definition(DEFAULT_THEME_JSON.as_bytes())?;
        let default = ThemeRecord {
            metadata: ThemeMetadata {
                theme_id: DEFAULT_THEME_ID.to_owned(),
                name: catalog_name(default_definition.name.as_deref(), "Git-Ramus Default"),
                plugin_id: HOST_THEME_PLUGIN_ID.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                density: default_definition
                    .density
                    .unwrap_or(ThemeDensity::Comfortable),
            },
            definition: default_definition,
        };
        upsert_theme(&database, &default.metadata, &default.definition, true)?;

        let mut themes = vec![default];
        for descriptor in plugins.descriptors() {
            let Some(contribution) = descriptor.manifest.contributions.theme.as_ref() else {
                continue;
            };
            if themes
                .iter()
                .any(|theme| theme.metadata.theme_id == contribution.theme_id)
            {
                continue;
            }
            match load_plugin_theme(descriptor, contribution) {
                Ok(definition) if definition.theme_id == contribution.theme_id => {
                    let metadata = ThemeMetadata {
                        theme_id: definition.theme_id.clone(),
                        name: catalog_name(definition.name.as_deref(), &descriptor.manifest.name),
                        plugin_id: descriptor.manifest.id.clone(),
                        version: descriptor.manifest.version.clone(),
                        density: definition.density.unwrap_or(ThemeDensity::Comfortable),
                    };
                    upsert_theme(&database, &metadata, &definition, true)?;
                    themes.push(ThemeRecord {
                        metadata,
                        definition,
                    });
                }
                Ok(_) => upsert_invalid_theme(
                    &database,
                    descriptor,
                    contribution,
                    ThemeLoadError::IdMismatch,
                )?,
                Err(reason) => upsert_invalid_theme(&database, descriptor, contribution, reason)?,
            }
        }
        themes[1..].sort_by(|left, right| left.metadata.theme_id.cmp(&right.metadata.theme_id));
        let manager = Self {
            database,
            themes: Arc::new(themes),
        };
        let active = manager.read_active_theme_id()?;
        if active.as_deref().and_then(|id| manager.find(id)).is_none() {
            manager.persist_active_theme(DEFAULT_THEME_ID)?;
        }
        Ok(manager)
    }

    pub fn list(&self) -> Vec<ThemeMetadata> {
        self.themes
            .iter()
            .map(|theme| theme.metadata.clone())
            .collect()
    }

    pub fn current(&self) -> Result<ThemeState, AppError> {
        let active = self.read_active_theme_id()?;
        let selected = active.as_deref().and_then(|id| self.find(id));
        let selected = match selected {
            Some(theme) => theme,
            None => {
                self.persist_active_theme(DEFAULT_THEME_ID)?;
                self.find(DEFAULT_THEME_ID)
                    .ok_or_else(|| AppError::NotFound("host default theme".to_owned()))?
            }
        };
        Ok(theme_state(selected))
    }

    pub fn activate(&self, theme_id: &str) -> Result<ThemeState, AppError> {
        let selected = self
            .find(theme_id)
            .or_else(|| self.find(DEFAULT_THEME_ID))
            .ok_or_else(|| AppError::NotFound("host default theme".to_owned()))?;
        self.persist_active_theme(&selected.metadata.theme_id)?;
        Ok(theme_state(selected))
    }

    fn find(&self, theme_id: &str) -> Option<&ThemeRecord> {
        self.themes
            .iter()
            .find(|theme| theme.metadata.theme_id == theme_id)
    }

    fn read_active_theme_id(&self) -> Result<Option<String>, AppError> {
        self.database.with_connection(|connection| {
            connection.query_row(
                "SELECT active_theme_id FROM global_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
        })
    }

    fn persist_active_theme(&self, theme_id: &str) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE global_settings SET active_theme_id=?1,updated_at=?2 WHERE id=1",
                rusqlite::params![theme_id, Utc::now().to_rfc3339()],
            )
        })?;
        if changed == 0 {
            return Err(AppError::NotFound("global settings".to_owned()));
        }
        Ok(())
    }
}

fn theme_state(theme: &ThemeRecord) -> ThemeState {
    ThemeState {
        active_theme_id: theme.metadata.theme_id.clone(),
        theme: theme.definition.clone(),
    }
}

fn load_plugin_theme(
    descriptor: &PluginDescriptor,
    contribution: &ThemeContribution,
) -> Result<ThemeDefinition, ThemeLoadError> {
    let definition_path = contribution
        .definition_path()
        .map_err(|_| ThemeLoadError::ReadFailed)?;
    let path = descriptor.root_path().join(definition_path);
    let canonical_path = path
        .canonicalize()
        .map_err(|_| ThemeLoadError::ReadFailed)?;
    if !canonical_path.starts_with(descriptor.root_path()) || !canonical_path.is_file() {
        return Err(ThemeLoadError::ReadFailed);
    }
    let metadata = canonical_path
        .metadata()
        .map_err(|_| ThemeLoadError::ReadFailed)?;
    if metadata.len() > MAX_THEME_BYTES {
        return Err(ThemeLoadError::InvalidSchema);
    }
    let bytes = std::fs::read(canonical_path).map_err(|_| ThemeLoadError::ReadFailed)?;
    parse_plugin_definition(&bytes)
}

fn parse_definition(bytes: &[u8]) -> Result<ThemeDefinition, AppError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let definition: ThemeDefinition = serde_json::from_value(value)?;
    validate_definition(&definition)?;
    Ok(definition)
}

fn parse_plugin_definition(bytes: &[u8]) -> Result<ThemeDefinition, ThemeLoadError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| ThemeLoadError::InvalidJson)?;
    let definition: ThemeDefinition =
        serde_json::from_value(value).map_err(|_| ThemeLoadError::InvalidSchema)?;
    validate_definition(&definition).map_err(|_| ThemeLoadError::InvalidSchema)?;
    Ok(definition)
}

fn validate_definition(definition: &ThemeDefinition) -> Result<(), AppError> {
    if !is_theme_id(&definition.theme_id) {
        return invalid_theme();
    }
    if let Some(name) = &definition.name {
        if !is_safe_text(name, 64) {
            return invalid_theme();
        }
    }
    if let Some(colors) = &definition.colors {
        for color in [
            &colors.background,
            &colors.surface,
            &colors.surface_raised,
            &colors.text,
            &colors.text_muted,
            &colors.border,
            &colors.primary,
            &colors.secondary,
            &colors.accent,
            &colors.success,
            &colors.warning,
            &colors.danger,
            &colors.focus_ring,
        ]
        .into_iter()
        .flatten()
        {
            if !is_safe_color(color) {
                return invalid_theme();
            }
        }
    }
    if let Some(typography) = &definition.typography {
        if let Some(family) = &typography.font_family {
            if family.is_empty()
                || family.chars().count() > 128
                || !is_safe_token(family)
                || !family.chars().all(|character| {
                    character.is_alphanumeric()
                        || character.is_whitespace()
                        || matches!(character, ',' | '-' | '_' | '\'' | '"')
                })
            {
                return invalid_theme();
            }
        }
        validate_optional_length(typography.font_size.as_ref(), 8.0, 72.0, false)?;
        validate_optional_length(typography.line_height.as_ref(), 1.0, 3.0, false)?;
        validate_optional_length(typography.letter_spacing.as_ref(), -4.0, 16.0, true)?;
        if let Some(weight) = &typography.font_weight {
            let valid = match weight {
                ThemeFontWeight::Number(value) => (100..=900).contains(value),
                ThemeFontWeight::Text(value) => matches!(value.as_str(), "normal" | "bold"),
            };
            if !valid {
                return invalid_theme();
            }
        }
    }
    if let Some(spacing) = &definition.spacing {
        for value in [
            &spacing.unit,
            &spacing.xs,
            &spacing.sm,
            &spacing.md,
            &spacing.lg,
            &spacing.xl,
        ] {
            validate_optional_length(value.as_ref(), 0.0, 128.0, false)?;
        }
    }
    if let Some(shape) = &definition.shape {
        for value in [
            &shape.radius,
            &shape.radius_sm,
            &shape.radius_md,
            &shape.radius_lg,
        ] {
            validate_optional_length(value.as_ref(), 0.0, 64.0, false)?;
        }
    }
    if let Some(elevation) = &definition.elevation {
        for shadow in [
            &elevation.none,
            &elevation.sm,
            &elevation.md,
            &elevation.lg,
            &elevation.level1,
            &elevation.level2,
            &elevation.level3,
        ]
        .into_iter()
        .flatten()
        {
            if shadow.is_empty()
                || shadow.chars().count() > 128
                || !is_safe_token(shadow)
                || !shadow.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || character.is_ascii_whitespace()
                        || matches!(character, '.' | '-' | '#' | ',' | '(' | ')' | '%')
                })
            {
                return invalid_theme();
            }
        }
    }
    if let Some(motion) = &definition.motion {
        for duration in [
            &motion.duration_fast,
            &motion.duration_normal,
            &motion.duration_slow,
        ]
        .into_iter()
        .flatten()
        {
            if !is_safe_duration(duration) {
                return invalid_theme();
            }
        }
        if let Some(easing) = &motion.easing {
            if !matches!(
                easing.as_str(),
                "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out"
            ) {
                return invalid_theme();
            }
        }
    }
    Ok(())
}

fn validate_optional_length(
    value: Option<&ThemeLength>,
    minimum: f64,
    maximum: f64,
    allow_negative: bool,
) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let (number, unit) = match value {
        ThemeLength::Number(number) => (*number, ""),
        ThemeLength::Text(value) => parse_length(value).ok_or_else(invalid_theme_error)?,
    };
    if !number.is_finite() || (!allow_negative && number < 0.0) {
        return invalid_theme();
    }
    let valid = match unit {
        "" | "px" => (minimum..=maximum).contains(&number),
        "rem" | "em" => (minimum / 16.0..=maximum / 16.0).contains(&number),
        "%" if minimum >= 0.0 => (0.0..=100.0).contains(&number),
        _ => false,
    };
    if valid { Ok(()) } else { invalid_theme() }
}

fn parse_length(value: &str) -> Option<(f64, &str)> {
    if !is_safe_token(value) || value.is_empty() {
        return None;
    }
    let split = value
        .find(|character: char| !character.is_ascii_digit() && !matches!(character, '.' | '-'))
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    Some((number.parse().ok()?, unit))
}

fn is_safe_duration(value: &str) -> bool {
    if !is_safe_token(value) {
        return false;
    }
    let milliseconds = value
        .strip_suffix("ms")
        .and_then(|number| number.parse::<f64>().ok())
        .or_else(|| {
            value
                .strip_suffix('s')
                .and_then(|number| number.parse::<f64>().ok())
                .map(|seconds| seconds * 1000.0)
        });
    matches!(milliseconds, Some(value) if value.is_finite() && (0.0..=2_000.0).contains(&value))
}

fn is_safe_color(value: &str) -> bool {
    if !is_safe_token(value) {
        return false;
    }
    matches!(value, "transparent" | "currentColor")
        || matches!(value.len(), 4 | 5 | 7 | 9)
            && value.starts_with('#')
            && value[1..]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
}

fn is_safe_token(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    let compact = lowered
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    !value.chars().any(char::is_control)
        && !value.contains(['<', '>', ';', '{', '}'])
        && !["url(", "@import", "javascript:", "expression("]
            .iter()
            .any(|marker| compact.contains(marker))
}

fn is_theme_id(value: &str) -> bool {
    let mut contains_separator = false;
    let mut previous_was_separator = true;
    for character in value.chars() {
        match character {
            '.' | '-' => {
                if previous_was_separator {
                    return false;
                }
                contains_separator = true;
                previous_was_separator = true;
            }
            'a'..='z' | '0'..='9' => previous_was_separator = false,
            _ => return false,
        }
    }
    contains_separator && !previous_was_separator
}

fn invalid_theme<T>() -> Result<T, AppError> {
    Err(invalid_theme_error())
}

fn invalid_theme_error() -> AppError {
    AppError::InvalidInput("theme definition is invalid".to_owned())
}

fn catalog_name(definition_name: Option<&str>, manifest_name: &str) -> String {
    definition_name
        .filter(|name| is_safe_text(name, 64))
        .or_else(|| is_safe_text(manifest_name, 64).then_some(manifest_name))
        .unwrap_or(UNNAMED_THEME_NAME)
        .to_owned()
}

fn invalidate_plugin_themes(database: &Database) -> Result<(), AppError> {
    database.with_connection(|connection| {
        connection
            .execute(
                "UPDATE themes SET definition_json=?1,is_valid=0,updated_at=?2 WHERE plugin_id<>?3",
                rusqlite::params![
                    invalid_reason_json(STALE_THEME_REASON),
                    Utc::now().to_rfc3339(),
                    HOST_THEME_PLUGIN_ID
                ],
            )
            .map(|_| ())
    })
}

fn upsert_invalid_theme(
    database: &Database,
    descriptor: &PluginDescriptor,
    contribution: &ThemeContribution,
    reason: ThemeLoadError,
) -> Result<(), AppError> {
    let metadata = ThemeMetadata {
        theme_id: contribution.theme_id.clone(),
        name: catalog_name(None, &descriptor.manifest.name),
        plugin_id: descriptor.manifest.id.clone(),
        version: descriptor.manifest.version.clone(),
        density: ThemeDensity::Comfortable,
    };
    upsert_theme_json(
        database,
        &metadata,
        &invalid_reason_json(reason.reason_code()),
        false,
    )
}

fn invalid_reason_json(reason: &str) -> String {
    serde_json::json!({ "invalidReason": reason }).to_string()
}

fn upsert_theme(
    database: &Database,
    metadata: &ThemeMetadata,
    definition: &ThemeDefinition,
    is_valid: bool,
) -> Result<(), AppError> {
    let definition_json = serde_json::to_string(definition)?;
    upsert_theme_json(database, metadata, &definition_json, is_valid)
}

fn upsert_theme_json(
    database: &Database,
    metadata: &ThemeMetadata,
    definition_json: &str,
    is_valid: bool,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    database.with_connection(|connection| {
        connection
            .execute(
                "INSERT INTO themes(theme_id,plugin_id,version,definition_json,is_valid,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(theme_id) DO UPDATE SET plugin_id=excluded.plugin_id,version=excluded.version,definition_json=excluded.definition_json,is_valid=excluded.is_valid,updated_at=excluded.updated_at",
                rusqlite::params![
                    metadata.theme_id,
                    metadata.plugin_id,
                    metadata.version,
                    definition_json,
                    is_valid,
                    now
                ],
            )
            .map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::catalog_name;

    #[test]
    fn unsafe_missing_definition_name_uses_a_stable_safe_catalog_fallback() {
        assert_eq!(catalog_name(None, "Compact Theme"), "Compact Theme");
        let utf16_too_long = "😀".repeat(33);
        assert_eq!(
            catalog_name(Some(&utf16_too_long), "Compact Theme"),
            "Compact Theme"
        );
        assert_eq!(catalog_name(None, &utf16_too_long), "Unnamed theme");
        for unsafe_name in [
            "",
            "<style>body{color:red}</style>",
            "unsafe\0name",
            "url\u{00a0}(https://evil.test)",
            &"x".repeat(65),
        ] {
            assert_eq!(catalog_name(None, unsafe_name), "Unnamed theme");
        }
    }
}
