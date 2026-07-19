use std::fs;
use std::path::Path;

use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::plugins::PluginRegistry;
use git_ramus_desktop_lib::themes::{DEFAULT_THEME_ID, ThemeManager};
use serde_json::{Value, json};
use tempfile::tempdir;

const COMPACT_PLUGIN_ID: &str = "git-ramus.compact-theme";
const COMPACT_THEME_ID: &str = "git-ramus.theme.compact";
const BUILTIN_COMPACT_MANIFEST: &str =
    include_str!("../../../../plugins/builtin-compact-theme/plugin.json");
const BUILTIN_COMPACT_THEME: &str =
    include_str!("../../../../plugins/builtin-compact-theme/theme.json");

fn compact_theme() -> Value {
    json!({
        "themeId": COMPACT_THEME_ID,
        "name": "Compact",
        "colors": {
            "background": "#07111f",
            "surface": "#0d1b2a",
            "surfaceRaised": "#13283d",
            "text": "#e6f2ff",
            "textMuted": "#8aa4bd",
            "border": "#27445f",
            "primary": "#38bdf8",
            "accent": "#22d3ee",
            "danger": "#fb7185",
            "focusRing": "#7dd3fc"
        },
        "typography": {
            "fontFamily": "Inter, system-ui, sans-serif",
            "fontSize": 13,
            "lineHeight": 1.35,
            "fontWeight": 400,
            "letterSpacing": 0
        },
        "spacing": { "unit": 4, "xs": 2, "sm": 6, "md": 10, "lg": 14, "xl": 20 },
        "shape": { "radius": 5, "radiusSm": 3, "radiusMd": 5, "radiusLg": 8 },
        "elevation": { "none": "none", "sm": "0 1px 2px #0003", "md": "0 4px 12px #0004" },
        "motion": {
            "durationFast": "90ms",
            "durationNormal": "140ms",
            "durationSlow": "220ms",
            "easing": "ease-out"
        },
        "density": "compact"
    })
}

fn write_theme_plugin(root: &Path, theme: &Value) {
    let plugin = root.join(COMPACT_PLUGIN_ID);
    fs::create_dir_all(&plugin).expect("plugin directory creates");
    fs::write(
        plugin.join("plugin.json"),
        format!(
            r#"{{"schemaVersion":1,"id":"{COMPACT_PLUGIN_ID}","name":"Compact Theme","version":"0.1.0","publisher":"git-ramus","description":"Compact global theme","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{{"ui":"ui.html"}},"contributions":{{"navigation":[],"theme":{{"themeId":"{COMPACT_THEME_ID}","definition":"theme.json"}}}},"permissions":[]}}"#
        ),
    )
    .expect("manifest writes");
    fs::write(plugin.join("ui.html"), "<main>Compact theme preview</main>").expect("UI writes");
    fs::write(
        plugin.join("theme.json"),
        serde_json::to_vec(theme).expect("theme serializes"),
    )
    .expect("theme writes");
}

#[test]
fn discovers_a_valid_theme_and_persists_only_validated_definition_metadata() {
    let directory = tempdir().expect("temp directory creates");
    write_theme_plugin(directory.path(), &compact_theme());
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");

    let manager = ThemeManager::discover(database.clone(), &registry).expect("themes discover");
    let catalog = manager.list();
    let compact = catalog
        .iter()
        .find(|theme| theme.theme_id == COMPACT_THEME_ID)
        .expect("compact theme is listed");

    assert_eq!(
        catalog.first().map(|theme| theme.theme_id.as_str()),
        Some(DEFAULT_THEME_ID)
    );
    assert_eq!(compact.plugin_id, COMPACT_PLUGIN_ID);
    assert_eq!(compact.name, "Compact");
    assert_eq!(compact.density.as_str(), "compact");
    let stored: (String, bool) = database
        .with_connection(|connection| {
            connection.query_row(
                "SELECT definition_json,is_valid FROM themes WHERE theme_id=?1",
                [COMPACT_THEME_ID],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .expect("stored theme reads");
    let stored_definition: Value = serde_json::from_str(&stored.0).expect("stored JSON parses");
    assert_eq!(stored_definition["themeId"], COMPACT_THEME_ID);
    assert!(stored.1);
    assert!(!stored.0.contains("theme.json"));
}

#[test]
fn discovers_the_shipped_compact_theme_definition() {
    let directory = tempdir().expect("temp directory creates");
    let plugin = directory.path().join(COMPACT_PLUGIN_ID);
    fs::create_dir_all(&plugin).expect("plugin directory creates");
    fs::write(plugin.join("plugin.json"), BUILTIN_COMPACT_MANIFEST).expect("manifest writes");
    fs::write(plugin.join("ui.html"), "<main>Compact preview</main>").expect("UI writes");
    fs::write(plugin.join("theme.json"), BUILTIN_COMPACT_THEME).expect("theme writes");
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");

    let manager = ThemeManager::discover(database, &registry).expect("themes discover");

    assert!(
        manager
            .list()
            .iter()
            .any(|theme| theme.theme_id == COMPACT_THEME_ID && theme.density.as_str() == "compact")
    );
}

#[test]
fn rejects_unknown_executable_and_out_of_range_theme_tokens() {
    let cases = [
        (
            "unknown token",
            json!({ "colors": { "arbitrary": "#fff" } }),
        ),
        ("raw CSS", json!({ "css": "body { color: red }" })),
        (
            "URL",
            json!({ "colors": { "background": "url(https://evil.test/a.png)" } }),
        ),
        (
            "HTML",
            json!({ "typography": { "fontFamily": "<style>body{color:red}</style>" } }),
        ),
        (
            "expression",
            json!({ "motion": { "easing": "expression(alert(1))" } }),
        ),
        (
            "unicode whitespace URL",
            json!({ "name": "url\u{00a0}(https://evil.test)" }),
        ),
        ("out of range", json!({ "spacing": { "unit": 10000 } })),
    ];

    for (name, mutation) in cases {
        let directory = tempdir().expect("temp directory creates");
        let mut definition = compact_theme();
        merge_object(&mut definition, mutation);
        write_theme_plugin(directory.path(), &definition);
        let database = Database::open_in_memory().expect("database opens");
        let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");

        let manager = ThemeManager::discover(database.clone(), &registry).expect("host survives");

        assert_eq!(manager.list().len(), 1, "unsafe case was listed: {name}");
        let stored: (String, bool) = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT definition_json,is_valid FROM themes WHERE theme_id=?1",
                    [COMPACT_THEME_ID],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .expect("invalid metadata is recorded");
        assert_eq!(
            stored,
            (
                r#"{"invalidReason":"theme.definition.invalid-schema"}"#.to_owned(),
                false
            ),
            "unsafe case persisted: {name}"
        );
    }
}

#[test]
fn records_stable_redacted_reason_codes_for_invalid_theme_definitions() {
    let cases = [
        ("missing", "theme.definition.read-failed"),
        ("invalid-json", "theme.definition.invalid-json"),
        ("invalid-schema", "theme.definition.invalid-schema"),
        ("id-mismatch", "theme.definition.id-mismatch"),
    ];

    for (case, expected_reason) in cases {
        let directory = tempdir().expect("temp directory creates");
        write_theme_plugin(directory.path(), &compact_theme());
        let definition_path = directory.path().join(COMPACT_PLUGIN_ID).join("theme.json");
        match case {
            "missing" => fs::remove_file(&definition_path).expect("definition removes"),
            "invalid-json" => fs::write(&definition_path, r#"{"themeId":"raw-secret""#)
                .expect("invalid JSON writes"),
            "invalid-schema" => fs::write(
                &definition_path,
                format!(
                    r#"{{"themeId":"{COMPACT_THEME_ID}","colors":{{"background":"url(https://evil.test/raw-secret.png)"}}}}"#
                ),
            )
            .expect("unsafe schema writes"),
            "id-mismatch" => fs::write(
                &definition_path,
                r#"{"themeId":"git-ramus.theme.raw-secret","name":"Mismatch"}"#,
            )
            .expect("mismatched definition writes"),
            _ => unreachable!("covered fixture"),
        }
        let database = Database::open_in_memory().expect("database opens");
        let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");

        let manager = ThemeManager::discover(database.clone(), &registry).expect("host survives");
        let stored: (String, String, String, bool) = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT plugin_id,version,definition_json,is_valid FROM themes WHERE theme_id=?1",
                    [COMPACT_THEME_ID],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            })
            .expect("invalid theme metadata reads");
        let diagnostic: Value = serde_json::from_str(&stored.2).expect("diagnostic JSON parses");

        assert_eq!(
            manager.list().len(),
            1,
            "invalid theme entered catalog: {case}"
        );
        assert_eq!(stored.0, COMPACT_PLUGIN_ID);
        assert_eq!(stored.1, "0.1.0");
        assert!(!stored.3);
        assert_eq!(diagnostic, json!({ "invalidReason": expected_reason }));
        assert_eq!(diagnostic.as_object().map(serde_json::Map::len), Some(1));
        assert!(!stored.2.contains("raw-secret"));
        assert!(!stored.2.contains("evil.test"));
        assert!(
            !stored
                .2
                .contains(&directory.path().to_string_lossy().to_string())
        );
    }
}

#[test]
fn marks_disappeared_plugin_themes_with_a_stable_stale_reason() {
    let directory = tempdir().expect("temp directory creates");
    write_theme_plugin(directory.path(), &compact_theme());
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");
    ThemeManager::discover(database.clone(), &registry).expect("themes discover");
    let empty = tempdir().expect("empty plugin root creates");
    let empty_registry = PluginRegistry::discover(empty.path()).expect("empty registry discovers");

    ThemeManager::discover(database.clone(), &empty_registry).expect("stale themes invalidate");
    let stored: (String, String, bool) = database
        .with_connection(|connection| {
            connection.query_row(
                "SELECT plugin_id,definition_json,is_valid FROM themes WHERE theme_id=?1",
                [COMPACT_THEME_ID],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .expect("stale theme reads");

    assert_eq!(stored.0, COMPACT_PLUGIN_ID);
    assert_eq!(
        serde_json::from_str::<Value>(&stored.1).expect("stale diagnostic parses"),
        json!({ "invalidReason": "theme.plugin.stale" })
    );
    assert!(!stored.2);
}

#[test]
fn activation_persists_and_unknown_activation_falls_back_to_host_default() {
    let directory = tempdir().expect("temp directory creates");
    write_theme_plugin(directory.path(), &compact_theme());
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");
    let manager = ThemeManager::discover(database.clone(), &registry).expect("themes discover");

    let active = manager
        .activate(COMPACT_THEME_ID)
        .expect("compact activates");
    assert_eq!(active.active_theme_id, COMPACT_THEME_ID);
    assert_eq!(active.theme.theme_id, COMPACT_THEME_ID);
    assert_eq!(
        stored_active_theme(&database).as_deref(),
        Some(COMPACT_THEME_ID)
    );

    let fallback = manager
        .activate("git-ramus.theme.unknown")
        .expect("invalid activation safely falls back");
    assert_eq!(fallback.active_theme_id, DEFAULT_THEME_ID);
    assert_eq!(fallback.theme.theme_id, DEFAULT_THEME_ID);
    assert_eq!(
        stored_active_theme(&database).as_deref(),
        Some(DEFAULT_THEME_ID)
    );
}

#[test]
fn startup_repairs_unknown_or_no_longer_valid_active_theme_ids() {
    let directory = tempdir().expect("temp directory creates");
    write_theme_plugin(directory.path(), &compact_theme());
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");
    let manager = ThemeManager::discover(database.clone(), &registry).expect("themes discover");
    manager
        .activate(COMPACT_THEME_ID)
        .expect("compact activates");

    let mut invalid = compact_theme();
    invalid["themeId"] = json!("git-ramus.theme.mismatch");
    write_theme_plugin(directory.path(), &invalid);
    let registry = PluginRegistry::discover(directory.path()).expect("plugins rediscover");
    let repaired = ThemeManager::discover(database.clone(), &registry).expect("host survives");
    assert_eq!(
        repaired
            .current()
            .expect("current theme resolves")
            .active_theme_id,
        DEFAULT_THEME_ID
    );
    assert_eq!(
        stored_active_theme(&database).as_deref(),
        Some(DEFAULT_THEME_ID)
    );

    database
        .with_connection(|connection| {
            connection.execute_batch("PRAGMA foreign_keys=OFF")?;
            connection.execute(
                "UPDATE global_settings SET active_theme_id='git-ramus.theme.missing' WHERE id=1",
                [],
            )?;
            connection.execute_batch("PRAGMA foreign_keys=ON")
        })
        .expect("corrupt active id writes");
    let repaired = ThemeManager::discover(database.clone(), &registry).expect("host repairs state");
    assert_eq!(
        repaired
            .current()
            .expect("current theme resolves")
            .active_theme_id,
        DEFAULT_THEME_ID
    );
    assert_eq!(
        stored_active_theme(&database).as_deref(),
        Some(DEFAULT_THEME_ID)
    );
}

#[test]
fn plugin_cannot_replace_or_invalidate_the_host_default_theme_id() {
    let directory = tempdir().expect("temp directory creates");
    let plugin = directory.path().join(COMPACT_PLUGIN_ID);
    fs::create_dir_all(&plugin).expect("plugin directory creates");
    fs::write(
        plugin.join("plugin.json"),
        format!(
            r#"{{"schemaVersion":1,"id":"{COMPACT_PLUGIN_ID}","name":"Collision","version":"0.1.0","publisher":"git-ramus","description":"Collision fixture","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{{"ui":"ui.html"}},"contributions":{{"navigation":[],"theme":{{"themeId":"{DEFAULT_THEME_ID}","definition":"theme.json"}}}},"permissions":[]}}"#
        ),
    )
    .expect("manifest writes");
    fs::write(plugin.join("ui.html"), "<main>Collision</main>").expect("UI writes");
    fs::write(
        plugin.join("theme.json"),
        format!(r##"{{"themeId":"{DEFAULT_THEME_ID}","colors":{{"background":"#ffffff"}}}}"##),
    )
    .expect("theme writes");
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");

    let manager = ThemeManager::discover(database.clone(), &registry).expect("host survives");
    let stored: (String, bool) = database
        .with_connection(|connection| {
            connection.query_row(
                "SELECT plugin_id,is_valid FROM themes WHERE theme_id=?1",
                [DEFAULT_THEME_ID],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .expect("default metadata reads");

    assert_eq!(manager.list().len(), 1);
    assert_eq!(stored, ("git-ramus.host".to_owned(), true));
}

#[test]
fn invalid_plugin_definition_cannot_mark_the_host_default_theme_invalid() {
    let directory = tempdir().expect("temp directory creates");
    let plugin = directory.path().join(COMPACT_PLUGIN_ID);
    fs::create_dir_all(&plugin).expect("plugin directory creates");
    fs::write(
        plugin.join("plugin.json"),
        format!(
            r#"{{"schemaVersion":1,"id":"{COMPACT_PLUGIN_ID}","name":"Collision","version":"0.1.0","publisher":"git-ramus","description":"Collision fixture","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{{"ui":"ui.html"}},"contributions":{{"navigation":[],"theme":{{"themeId":"{DEFAULT_THEME_ID}","definition":"theme.json"}}}},"permissions":[]}}"#
        ),
    )
    .expect("manifest writes");
    fs::write(plugin.join("ui.html"), "<main>Collision</main>").expect("UI writes");
    fs::write(
        plugin.join("theme.json"),
        format!(r#"{{"themeId":"{DEFAULT_THEME_ID}","css":"body{{color:red}}"}}"#),
    )
    .expect("theme writes");
    let database = Database::open_in_memory().expect("database opens");
    let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");

    ThemeManager::discover(database.clone(), &registry).expect("host survives");
    let stored: (String, bool) = database
        .with_connection(|connection| {
            connection.query_row(
                "SELECT plugin_id,is_valid FROM themes WHERE theme_id=?1",
                [DEFAULT_THEME_ID],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .expect("default metadata reads");

    assert_eq!(stored, ("git-ramus.host".to_owned(), true));
}

fn stored_active_theme(database: &Database) -> Option<String> {
    database
        .with_connection(|connection| {
            connection.query_row(
                "SELECT active_theme_id FROM global_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
        })
        .expect("active theme reads")
}

fn merge_object(target: &mut Value, mutation: Value) {
    let Some(target) = target.as_object_mut() else {
        panic!("target is an object");
    };
    let Some(mutation) = mutation.as_object() else {
        panic!("mutation is an object");
    };
    for (key, value) in mutation {
        target.insert(key.clone(), value.clone());
    }
}
