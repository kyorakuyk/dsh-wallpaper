#[path = "../src/appearance/mod.rs"]
mod appearance;

use appearance::{
    AppearancePaths, AppearanceRepository, AppearanceSlot, AssetMediaType, AssetOrigin,
    AssetRecord, AssetStatus, InsertAssetOutcome, ThemeAssetLink, ThemeFileRecord, ThemeRecord,
    ThemeSource,
};
use tempfile::tempdir;

fn theme(id: &str, version: &str) -> ThemeRecord {
    ThemeRecord {
        id: id.into(),
        version: version.into(),
        name: id.into(),
        author: None,
        description: None,
        preview: None,
        source: ThemeSource::User,
        manifest_path: format!("library/themes/{id}/{version}/theme.json"),
        readonly: false,
        installed_at: Some(1),
        baseline_id: "official.deepsea".into(),
        baseline_version: "1.0.0".into(),
    }
}

fn loose_asset(id: &str, hash_byte: char, slot: AppearanceSlot) -> AssetRecord {
    AssetRecord {
        id: id.into(),
        sha256: hash_byte.to_string().repeat(64),
        media_type: AssetMediaType::Image,
        original_name: format!("{id}.webp"),
        object_path: format!("library/objects/sha256/{hash_byte}{hash_byte}/{id}.webp"),
        width: Some(1920),
        height: Some(1080),
        has_alpha: Some(false),
        status: AssetStatus::Classified,
        slots: vec![slot],
        origin: AssetOrigin::Loose,
        created_at: 1,
    }
}

#[test]
fn creates_the_documented_app_data_layout_and_database() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("dsh-wallpaper"));
    paths.create().unwrap();
    let repository = AppearanceRepository::open(&paths.catalog).unwrap();

    assert_eq!(repository.schema_version().unwrap(), 3);
    assert!(paths.objects_sha256.is_dir());
    assert!(paths.themes.is_dir());
    assert!(paths.inbox.is_dir());
    assert!(paths.previews.is_dir());
    assert!(paths.staging.is_dir());
    assert!(paths.exports.is_dir());
    assert!(paths.diagnostics.is_dir());
    assert_eq!(
        paths.object_path(&"A".repeat(64), "WEBP").unwrap(),
        paths
            .objects_sha256
            .join("aa")
            .join(format!("{}.webp", "a".repeat(64)))
    );
    assert!(paths.object_path("../escape", "webp").is_none());
    assert_eq!(
        paths.theme_manifest_path("user.deepsea", "1.0.0"),
        paths
            .themes
            .join("user.deepsea")
            .join("1.0.0")
            .join("theme.json")
    );
    assert_eq!(paths.root(), directory.path().join("dsh-wallpaper"));
    assert!(AppearancePaths::from_local_app_data().is_some());
}

#[test]
fn deduplicates_assets_by_sha256() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    let first = loose_asset("asset-a", 'a', AppearanceSlot::DesktopBackground);
    let second = AssetRecord {
        id: "asset-b".into(),
        ..first.clone()
    };

    assert!(matches!(
        repository.insert_asset(&first).unwrap(),
        InsertAssetOutcome::Inserted(_)
    ));
    match repository.insert_asset(&second).unwrap() {
        InsertAssetOutcome::Existing(existing) => assert_eq!(existing.id, "asset-a"),
        InsertAssetOutcome::Inserted(_) => panic!("duplicate content created another asset"),
    }
}

#[test]
fn hides_theme_private_and_inbox_assets_from_component_library() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    repository
        .insert_theme(&theme("user.deepsea", "1.0.0"))
        .unwrap();
    let visible = loose_asset("visible", 'a', AppearanceSlot::DesktopBackground);
    let inbox = AssetRecord {
        id: "inbox".into(),
        sha256: "b".repeat(64),
        status: AssetStatus::Inbox,
        ..visible.clone()
    };
    let private = AssetRecord {
        id: "private".into(),
        sha256: "c".repeat(64),
        origin: AssetOrigin::ThemePrivate {
            theme_id: "user.deepsea".into(),
            theme_version: "1.0.0".into(),
        },
        ..visible.clone()
    };
    repository.insert_asset(&visible).unwrap();
    repository.insert_asset(&inbox).unwrap();
    repository.insert_asset(&private).unwrap();

    let assets = repository
        .list_component_assets(AppearanceSlot::DesktopBackground)
        .unwrap();
    assert_eq!(assets, vec![visible]);
}

#[test]
fn activating_a_theme_clears_all_overrides_atomically() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    repository.insert_theme(&theme("theme-a", "1.0.0")).unwrap();
    repository.insert_theme(&theme("theme-b", "1.0.0")).unwrap();
    repository.activate_theme("theme-a", "1.0.0").unwrap();
    let asset = loose_asset("background", 'a', AppearanceSlot::DesktopBackground);
    repository.insert_asset(&asset).unwrap();
    repository
        .set_override(AppearanceSlot::DesktopBackground, &asset.id)
        .unwrap();

    repository.activate_theme("theme-b", "1.0.0").unwrap();

    assert_eq!(repository.active_theme().unwrap().unwrap().id, "theme-b");
    assert_eq!(
        repository
            .override_asset_id(AppearanceSlot::DesktopBackground)
            .unwrap(),
        None
    );

    repository.clear_override(None).unwrap();
}

#[test]
fn requires_official_themes_to_be_readonly() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    let invalid = ThemeRecord {
        source: ThemeSource::Official,
        readonly: false,
        ..theme("official.deepsea", "1.0.0")
    };
    assert!(repository.insert_theme(&invalid).is_err());
}

#[test]
fn classifies_an_inbox_asset_into_multiple_compatible_slots_atomically() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    let inbox = AssetRecord {
        status: AssetStatus::Inbox,
        slots: vec![],
        ..loose_asset("portrait", 'e', AppearanceSlot::DesktopBackground)
    };
    repository.insert_asset(&inbox).unwrap();

    let classified = repository
        .classify_asset(
            &inbox.id,
            &[
                AppearanceSlot::PersonaDeepseekFlash,
                AppearanceSlot::PersonaHarnessFlash,
            ],
        )
        .unwrap();
    assert_eq!(classified.status, AssetStatus::Classified);
    assert_eq!(classified.slots.len(), 2);
    assert_eq!(
        repository
            .list_component_assets(AppearanceSlot::PersonaDeepseekFlash)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn incompatible_classification_rolls_back_status_and_slots() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    let inbox = AssetRecord {
        status: AssetStatus::Inbox,
        slots: vec![],
        ..loose_asset("image", 'f', AppearanceSlot::DesktopBackground)
    };
    repository.insert_asset(&inbox).unwrap();

    assert!(repository
        .classify_asset(&inbox.id, &[AppearanceSlot::UiFont])
        .is_err());
    let stored = repository.asset_by_id(&inbox.id).unwrap();
    assert_eq!(stored.status, AssetStatus::Inbox);
    assert!(stored.slots.is_empty());
}

#[test]
fn failed_theme_activation_rolls_back_override_deletion() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    repository.insert_theme(&theme("theme-a", "1.0.0")).unwrap();
    repository.activate_theme("theme-a", "1.0.0").unwrap();
    let asset = loose_asset("background", 'a', AppearanceSlot::DesktopBackground);
    repository.insert_asset(&asset).unwrap();
    repository
        .set_override(AppearanceSlot::DesktopBackground, &asset.id)
        .unwrap();

    assert!(repository.activate_theme("missing", "1.0.0").is_err());

    assert_eq!(repository.active_theme().unwrap().unwrap().id, "theme-a");
    assert_eq!(
        repository
            .override_asset_id(AppearanceSlot::DesktopBackground)
            .unwrap()
            .as_deref(),
        Some("background")
    );
}

#[test]
fn rejects_theme_private_assets_as_manual_overrides() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    repository.insert_theme(&theme("theme-a", "1.0.0")).unwrap();
    let private = AssetRecord {
        origin: AssetOrigin::ThemePrivate {
            theme_id: "theme-a".into(),
            theme_version: "1.0.0".into(),
        },
        ..loose_asset("private", 'a', AppearanceSlot::DesktopBackground)
    };
    repository.insert_asset(&private).unwrap();
    assert!(repository
        .set_override(AppearanceSlot::DesktopBackground, &private.id)
        .is_err());
}

#[test]
fn failed_verified_theme_install_rolls_back_every_record() {
    let mut repository = AppearanceRepository::open_in_memory().unwrap();
    let record = theme("theme-install", "1.0.0");
    let asset = AssetRecord {
        origin: AssetOrigin::ThemePrivate {
            theme_id: record.id.clone(),
            theme_version: record.version.clone(),
        },
        ..loose_asset("private-background", 'd', AppearanceSlot::DesktopBackground)
    };
    let file = ThemeFileRecord {
        theme_id: record.id.clone(),
        theme_version: record.version.clone(),
        package_path: "scenes/background.webp".into(),
        sha256: asset.sha256.clone(),
        size: 100,
        asset_id: Some(asset.id.clone()),
    };
    let invalid_link = ThemeAssetLink {
        theme_id: record.id.clone(),
        theme_version: record.version.clone(),
        slot: AppearanceSlot::DesktopBackground,
        package_path: "scenes/not-in-files.webp".into(),
        asset_id: asset.id.clone(),
    };

    assert!(repository
        .install_verified_theme(&record, &[asset.clone()], &[file], &[invalid_link])
        .is_err());
    assert!(!repository
        .theme_exists(&record.id, &record.version)
        .unwrap());
    assert!(repository.asset_by_sha256(&asset.sha256).unwrap().is_none());

    // A clean retry must not collide with rows left by the failed transaction.
    repository.insert_theme(&record).unwrap();
}
