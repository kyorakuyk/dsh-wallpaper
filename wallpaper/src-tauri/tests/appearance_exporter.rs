#[path = "../src/appearance/mod.rs"]
mod appearance;

use std::collections::BTreeMap;
use std::fs;

use appearance::{
    AppearanceExporter, AppearanceImporter, AppearancePaths, AppearanceRepository, AppearanceSlot,
    AssetMediaType, AssetOrigin, AssetRecord, AssetStatus, ExportThemeMetadata,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn write_complete_theme(root: &std::path::Path, id: &str, baseline: (&str, &str)) {
    fs::create_dir_all(root.join("assets")).unwrap();
    let mut file_entries = Vec::new();
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
        file_entries
            .push(json!({ "path": path, "sha256": sha256(&content), "size": content.len() }));
        components.insert(slot.into(), json!({ "kind": "asset", "path": path }));
    }

    let frame_path = "assets/wake-frame.webp";
    let frame = format!("{id}-wake").into_bytes();
    fs::write(root.join(frame_path), &frame).unwrap();
    file_entries.push(json!({ "path": frame_path, "sha256": sha256(&frame), "size": frame.len() }));
    components.insert(
        "wake.sequence".into(),
        json!({ "kind": "sequence", "frames": [{ "path": frame_path, "durationMs": 500, "fadeMs": 100 }] }),
    );

    let skin_path = "assets/skin.json";
    let skin = format!("{{\"theme\":\"{id}\"}}").into_bytes();
    fs::write(root.join(skin_path), &skin).unwrap();
    file_entries.push(json!({ "path": skin_path, "sha256": sha256(&skin), "size": skin.len() }));
    components.insert(
        "chat.skin".into(),
        json!({ "kind": "skin", "definition": skin_path }),
    );

    let manifest = json!({
        "schemaVersion": 1,
        "kind": "theme",
        "id": id,
        "version": "1.0.0",
        "name": id,
        "compatibility": { "minAppVersion": "0.3.0" },
        "baseline": { "id": baseline.0, "version": baseline.1 },
        "components": components,
        "files": file_entries
    });
    fs::write(
        root.join("theme.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn loose_override(paths: &AppearancePaths) -> AssetRecord {
    let content = b"custom-override";
    let hash = sha256(content);
    let object = paths.object_path(&hash, "webp").unwrap();
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::write(&object, content).unwrap();
    AssetRecord {
        id: "override-background".into(),
        sha256: hash.clone(),
        media_type: AssetMediaType::Image,
        original_name: "override.webp".into(),
        object_path: format!("library/objects/sha256/{}/{hash}.webp", &hash[..2]),
        width: None,
        height: None,
        has_alpha: None,
        status: AssetStatus::Classified,
        slots: vec![AppearanceSlot::DesktopBackground],
        origin: AssetOrigin::Loose,
        created_at: 1,
    }
}

#[test]
fn exports_current_appearance_as_a_self_contained_importable_theme() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("app-data"));
    paths.create().unwrap();
    let mut repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let importer = AppearanceImporter::new(paths.clone()).unwrap();
    let source = directory.path().join("source-theme");
    write_complete_theme(&source, "user.source", ("user.source", "1.0.0"));
    importer.import_path(&source, &mut repository).unwrap();
    repository.activate_theme("user.source", "1.0.0").unwrap();
    let override_asset = loose_override(&paths);
    repository.insert_asset(&override_asset).unwrap();
    repository
        .set_override(AppearanceSlot::DesktopBackground, &override_asset.id)
        .unwrap();

    let destination = paths.exports.join("shared.dshwallpaper");
    let result = AppearanceExporter::new(paths.clone())
        .unwrap()
        .export_current_theme(
            &repository,
            &ExportThemeMetadata {
                id: "user.shared".into(),
                version: "1.0.0".into(),
                name: "Shared".into(),
                author: Some("Tester".into()),
                description: None,
            },
            &destination,
        )
        .unwrap();
    assert!(destination.is_file());
    assert_eq!(result.files, 9);

    let fresh_paths = AppearancePaths::new(directory.path().join("fresh-data"));
    fresh_paths.create().unwrap();
    let mut fresh_repository = AppearanceRepository::open(&fresh_paths.catalog).unwrap();
    AppearanceImporter::new(fresh_paths)
        .unwrap()
        .import_path(&destination, &mut fresh_repository)
        .unwrap();
    assert!(fresh_repository
        .theme_exists("user.shared", "1.0.0")
        .unwrap());
}

#[test]
fn exported_manifest_contains_every_slot_and_uses_the_override() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("data"));
    paths.create().unwrap();
    let mut repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let source = directory.path().join("theme");
    write_complete_theme(&source, "user.full", ("user.full", "1.0.0"));
    AppearanceImporter::new(paths.clone())
        .unwrap()
        .import_path(&source, &mut repository)
        .unwrap();
    repository.activate_theme("user.full", "1.0.0").unwrap();
    let override_asset = loose_override(&paths);
    repository.insert_asset(&override_asset).unwrap();
    repository
        .set_override(AppearanceSlot::DesktopBackground, &override_asset.id)
        .unwrap();

    let destination = paths.exports.join("inspect.dshwallpaper");
    AppearanceExporter::new(paths)
        .unwrap()
        .export_current_theme(
            &repository,
            &ExportThemeMetadata {
                id: "user.inspect".into(),
                version: "1.0.0".into(),
                name: "Inspect".into(),
                author: None,
                description: None,
            },
            &destination,
        )
        .unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(destination).unwrap()).unwrap();
    let manifest: Value = serde_json::from_reader(archive.by_name("theme.json").unwrap()).unwrap();
    let components = manifest["components"].as_object().unwrap();
    assert_eq!(components.len(), AppearanceSlot::ALL.len());
    assert_eq!(
        components["desktop.background"]["path"],
        "components/desktop-background/override.webp"
    );
    let files: BTreeMap<_, _> = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["path"].as_str().unwrap(),
                entry["sha256"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(files.len(), 9);
}

#[test]
fn export_fails_without_an_active_or_complete_theme() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("empty"));
    paths.create().unwrap();
    let repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let result = AppearanceExporter::new(paths.clone())
        .unwrap()
        .export_current_theme(
            &repository,
            &ExportThemeMetadata {
                id: "user.empty".into(),
                version: "1.0.0".into(),
                name: "Empty".into(),
                author: None,
                description: None,
            },
            &paths.exports.join("empty.dshwallpaper"),
        );
    assert!(result.is_err());
}
