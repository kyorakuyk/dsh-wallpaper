#[path = "../src/appearance/mod.rs"]
mod appearance;

use std::fs;
use std::path::Path;

use appearance::{
    AppearanceExporter, AppearanceImporter, AppearancePaths, AppearanceRepository, AppearanceSlot,
    AppearanceState, AssetMediaType, AssetOrigin, AssetRecord, AssetStatus, ExportThemeMetadataDto,
    ThemeRecord, ThemeSource,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn theme(id: &str, source: ThemeSource, readonly: bool) -> ThemeRecord {
    ThemeRecord {
        id: id.into(),
        version: "1.0.0".into(),
        name: id.into(),
        author: None,
        description: None,
        preview: None,
        source,
        manifest_path: format!("private/{id}/theme.json"),
        readonly,
        installed_at: Some(1),
        baseline_id: "official.deepsea".into(),
        baseline_version: "1.0.0".into(),
    }
}

fn asset(id: &str, status: AssetStatus, origin: AssetOrigin) -> AssetRecord {
    AssetRecord {
        id: id.into(),
        sha256: match id {
            "background" => "a".repeat(64),
            "inbox" => "b".repeat(64),
            _ => "c".repeat(64),
        },
        media_type: AssetMediaType::Image,
        original_name: format!("{id}.webp"),
        object_path: format!("private/objects/{id}.webp"),
        width: Some(1920),
        height: Some(1080),
        has_alpha: Some(false),
        status,
        slots: if id == "inbox" {
            vec![]
        } else {
            vec![AppearanceSlot::DesktopBackground]
        },
        origin,
        created_at: 1,
    }
}

fn state() -> AppearanceState {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    repository
        .insert_theme(&theme("official.deepsea", ThemeSource::Official, true))
        .unwrap();
    repository
        .insert_theme(&theme("user.night", ThemeSource::User, false))
        .unwrap();
    repository
        .insert_asset(&asset(
            "background",
            AssetStatus::Classified,
            AssetOrigin::Loose,
        ))
        .unwrap();
    repository
        .insert_asset(&asset("inbox", AssetStatus::Inbox, AssetOrigin::Loose))
        .unwrap();
    repository
        .insert_asset(&asset(
            "private",
            AssetStatus::Classified,
            AssetOrigin::ThemePrivate {
                theme_id: "user.night".into(),
                theme_version: "1.0.0".into(),
            },
        ))
        .unwrap();
    AppearanceState::new(repository)
}

fn assert_send_sync<T: Send + Sync>() {}

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn write_complete_theme(root: &Path, id: &str) {
    fs::create_dir_all(root.join("assets")).unwrap();
    let mut files = Vec::new();
    let mut components = serde_json::Map::new();

    for (slot, extension) in [
        ("desktop.background", "webp"),
        ("lockscreen.image", "png"),
        ("persona.deepseek.flash", "webp"),
        ("persona.deepseek.pro", "webp"),
        ("persona.harness.flash", "webp"),
        ("persona.harness.pro", "webp"),
        ("ui.font", "ttf"),
    ] {
        let path = format!("assets/{}.{}", slot.replace('.', "-"), extension);
        let content = format!("{id}-{slot}").into_bytes();
        fs::write(root.join(&path), &content).unwrap();
        files.push(json!({
            "path": path,
            "sha256": sha256(&content),
            "size": content.len()
        }));
        components.insert(slot.into(), json!({ "kind": "asset", "path": path }));
    }

    let wake_path = "assets/wake.webp";
    let wake = format!("{id}-wake").into_bytes();
    fs::write(root.join(wake_path), &wake).unwrap();
    files.push(json!({
        "path": wake_path,
        "sha256": sha256(&wake),
        "size": wake.len()
    }));
    components.insert(
        "wake.sequence".into(),
        json!({
            "kind": "sequence",
            "frames": [{ "path": wake_path, "durationMs": 500, "fadeMs": 100 }]
        }),
    );

    let skin_path = "assets/skin.json";
    let skin = br#"{"version":1}"#;
    fs::write(root.join(skin_path), skin).unwrap();
    files.push(json!({
        "path": skin_path,
        "sha256": sha256(skin),
        "size": skin.len()
    }));
    components.insert(
        "chat.skin".into(),
        json!({ "kind": "skin", "definition": skin_path }),
    );

    let manifest = json!({
        "schemaVersion": 1,
        "kind": "theme",
        "id": id,
        "version": "1.0.0",
        "name": "Command Test Theme",
        "compatibility": { "minAppVersion": "0.3.0" },
        "baseline": { "id": id, "version": "1.0.0" },
        "components": components,
        "files": files
    });
    fs::write(
        root.join("theme.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn lists_sanitized_theme_and_loose_asset_summaries() {
    let state = state();
    assert_send_sync::<AppearanceState>();
    assert!(state.get_state().unwrap().active_theme.is_none());
    let themes = state.list_themes().unwrap();
    assert_eq!(themes[0].source, "official");
    assert_eq!(themes[1].source, "user");

    let assets = state.list_assets(None).unwrap();
    assert_eq!(assets.len(), 2);
    assert!(assets.iter().any(|asset| asset.id == "background"));
    assert!(assets.iter().any(|asset| asset.id == "inbox"));
    assert!(assets.iter().all(|asset| asset.id != "private"));

    let filtered = state.list_assets(Some("desktop.background")).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "background");
}

#[test]
fn writes_return_a_complete_snapshot() {
    let state = state();
    let activated = state.activate_theme("official.deepsea", "1.0.0").unwrap();
    assert_eq!(activated.active_theme.unwrap().id, "official.deepsea");
    assert!(activated.overrides.is_empty());

    let overridden = state
        .set_override("desktop.background", "background")
        .unwrap();
    assert_eq!(overridden.overrides.len(), 1);
    assert_eq!(
        overridden
            .overrides
            .get("desktop.background")
            .map(String::as_str),
        Some("background")
    );

    let cleared = state.clear_override(None).unwrap();
    assert!(cleared.overrides.is_empty());
    assert_eq!(cleared.active_theme.unwrap().id, "official.deepsea");
}

#[test]
fn maps_failures_to_stable_errors_without_internal_details() {
    let state = state();
    let invalid_slot = state.list_assets(Some("C:/private/path")).unwrap_err();
    assert_eq!(invalid_slot.code, "APPEARANCE_INVALID_ARGUMENT");
    assert!(!invalid_slot.message.contains("C:/private/path"));

    let missing = state
        .set_override("desktop.background", "C:/secret/asset")
        .unwrap_err();
    assert_eq!(missing.code, "APPEARANCE_NOT_FOUND");
    assert!(!missing.message.contains("C:/secret/asset"));

    let missing_theme = state
        .activate_theme("C:/secret/theme", "1.0.0")
        .unwrap_err();
    assert_eq!(missing_theme.code, "APPEARANCE_NOT_FOUND");
    assert!(!missing_theme.message.contains("C:/secret/theme"));

    let invalid_classification = state
        .classify_asset("inbox", &["C:/secret/slot".into()])
        .unwrap_err();
    assert_eq!(invalid_classification.code, "APPEARANCE_INVALID_ARGUMENT");
    assert!(!invalid_classification.message.contains("C:/secret/slot"));

    let missing_exporter = state
        .export_current_theme(
            ExportThemeMetadataDto {
                id: "user.private".into(),
                version: "1.0.0".into(),
                name: "Private".into(),
                author: None,
                description: None,
            },
            "C:/secret/export.dshwallpaper",
        )
        .unwrap_err();
    assert_eq!(missing_exporter.code, "APPEARANCE_INTERNAL");
    assert!(!missing_exporter.message.contains("C:/secret"));
}

#[test]
fn with_io_supports_import_classification_and_self_contained_export() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("app-data"));
    paths.create().unwrap();
    let mut repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let importer = AppearanceImporter::new(paths.clone()).unwrap();
    let exporter = AppearanceExporter::new(paths.clone()).unwrap();

    let theme_source = directory.path().join("complete-theme");
    write_complete_theme(&theme_source, "user.command-source");
    importer
        .import_path(&theme_source, &mut repository)
        .unwrap();
    repository
        .activate_theme("user.command-source", "1.0.0")
        .unwrap();

    let loose_source = directory.path().join("custom-background.png");
    fs::write(&loose_source, b"command-custom-background").unwrap();
    let state = AppearanceState::with_io(repository, importer, exporter);
    let imported = state
        .import_paths(&[loose_source.to_string_lossy().into_owned()])
        .unwrap();
    assert_eq!(imported.results[0].kind, "inbox");

    let inbox = state
        .list_assets(None)
        .unwrap()
        .into_iter()
        .find(|asset| asset.status == "inbox")
        .unwrap();
    let classified = state
        .classify_asset(
            &inbox.id,
            &[
                "desktop.background".into(),
                "lockscreen.image".into(),
                "desktop.background".into(),
            ],
        )
        .unwrap();
    assert_eq!(classified.status, "classified");
    assert_eq!(
        classified.slots,
        vec!["desktop.background", "lockscreen.image"]
    );

    state
        .set_override("desktop.background", &classified.id)
        .unwrap();
    let destination = paths.exports.join("command-export.dshwallpaper");
    let exported = state
        .export_current_theme(
            ExportThemeMetadataDto {
                id: "user.command-export".into(),
                version: "1.0.0".into(),
                name: "Command Export".into(),
                author: Some("Tester".into()),
                description: Some("Exported through AppearanceState".into()),
            },
            &destination.to_string_lossy(),
        )
        .unwrap();
    assert_eq!(exported.files, AppearanceSlot::ALL.len());
    assert!(destination.is_file());
}

#[test]
fn export_failures_are_sanitized_at_the_command_boundary() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("empty-data"));
    paths.create().unwrap();
    let repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let state = AppearanceState::with_io(
        repository,
        AppearanceImporter::new(paths.clone()).unwrap(),
        AppearanceExporter::new(paths.clone()).unwrap(),
    );

    let secret_destination = directory.path().join("private").join("shared.dshwallpaper");
    let error = state
        .export_current_theme(
            ExportThemeMetadataDto {
                id: "user.shared".into(),
                version: "1.0.0".into(),
                name: "Shared".into(),
                author: None,
                description: None,
            },
            &secret_destination.to_string_lossy(),
        )
        .unwrap_err();
    assert_eq!(error.code, "APPEARANCE_EXPORT_INVALID");
    assert_eq!(error.message, "当前外观无法完整导出");
    assert!(!error
        .message
        .contains(&directory.path().to_string_lossy()[..]));

    let invalid_destination = state
        .export_current_theme(
            ExportThemeMetadataDto {
                id: "user.shared".into(),
                version: "1.0.0".into(),
                name: "Shared".into(),
                author: None,
                description: None,
            },
            "   ",
        )
        .unwrap_err();
    assert_eq!(invalid_destination.code, "APPEARANCE_INVALID_ARGUMENT");
}

#[test]
fn export_rejects_metadata_that_the_importer_cannot_install() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("metadata-data"));
    paths.create().unwrap();
    let mut repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let importer = AppearanceImporter::new(paths.clone()).unwrap();
    let source = directory.path().join("metadata-theme");
    write_complete_theme(&source, "user.metadata-source");
    importer.import_path(&source, &mut repository).unwrap();
    repository
        .activate_theme("user.metadata-source", "1.0.0")
        .unwrap();
    let state = AppearanceState::with_io(
        repository,
        importer,
        AppearanceExporter::new(paths.clone()).unwrap(),
    );

    for (id, version) in [
        ("User.Uppercase", "1.0.0"),
        ("user.valid", "latest"),
        ("../escape", "1.0.0"),
    ] {
        let destination = paths.exports.join(format!("{id}-invalid.dshwallpaper"));
        let error = state
            .export_current_theme(
                ExportThemeMetadataDto {
                    id: id.into(),
                    version: version.into(),
                    name: "Invalid Metadata".into(),
                    author: None,
                    description: None,
                },
                &destination.to_string_lossy(),
            )
            .unwrap_err();
        assert_eq!(error.code, "APPEARANCE_EXPORT_INVALID");
        assert!(!destination.exists());
    }
}
