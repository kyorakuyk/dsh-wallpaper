#[path = "../src/appearance/mod.rs"]
mod appearance;

use std::fs;
use std::io::Write;
use std::path::Path;

use appearance::{
    AppearanceImporter, AppearancePaths, AppearanceRepository, AppearanceSlot, AppearanceState,
    ImportLimits, ImportResult, ImportSource, DEFAULT_MAX_ASSET_BYTES,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

fn setup() -> (
    tempfile::TempDir,
    AppearancePaths,
    AppearanceRepository,
    AppearanceImporter,
) {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("app-data"));
    let repository = AppearanceRepository::open(&paths.catalog).unwrap_or_else(|_| {
        paths.create().unwrap();
        AppearanceRepository::open(&paths.catalog).unwrap()
    });
    let importer = AppearanceImporter::new(paths.clone()).unwrap();
    (directory, paths, repository, importer)
}

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn write_theme_folder(root: &Path, id: &str, content: &[u8]) {
    fs::create_dir_all(root.join("scenes")).unwrap();
    fs::write(root.join("scenes/background.webp"), content).unwrap();
    let hash = sha256(content);
    let manifest = json!({
        "schemaVersion": 1,
        "kind": "theme",
        "id": id,
        "version": "1.0.0",
        "name": "Test Theme",
        "compatibility": { "minAppVersion": "0.3.0" },
        "baseline": { "id": "official.deepsea", "version": "1.0.0" },
        "components": {
            "desktop.background": { "kind": "asset", "path": "scenes/background.webp" }
        },
        "files": [{ "path": "scenes/background.webp", "sha256": hash, "size": content.len() }]
    });
    fs::write(
        root.join("theme.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn imports_plain_folder_into_inbox_and_deduplicates_by_hash() {
    let (directory, paths, mut repository, importer) = setup();
    let source = directory.path().join("loose");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("fat-fish.png"), b"same-image").unwrap();

    let first = importer.import_path(&source, &mut repository).unwrap();
    assert_eq!(
        first,
        ImportResult::Inbox {
            source: ImportSource::Folder,
            imported: 1,
            deduplicated: 0,
        }
    );
    let assets = repository.list_library_assets(None).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].status.as_str(), "inbox");
    assert!(assets[0].slots.is_empty());
    assert!(paths
        .object_path(&assets[0].sha256, "png")
        .unwrap()
        .is_file());

    let second = importer.import_path(&source, &mut repository).unwrap();
    assert_eq!(
        second,
        ImportResult::Inbox {
            source: ImportSource::Folder,
            imported: 0,
            deduplicated: 1,
        }
    );
}

#[test]
fn installs_valid_theme_as_private_assets() {
    let (directory, paths, mut repository, importer) = setup();
    let source = directory.path().join("theme");
    write_theme_folder(&source, "user.ocean", b"private-background");

    let result = importer.import_path(&source, &mut repository).unwrap();
    assert_eq!(
        result,
        ImportResult::Theme {
            source: ImportSource::Folder,
            id: "user.ocean".into(),
            version: "1.0.0".into(),
            imported: 1,
            deduplicated: 0,
        }
    );
    assert!(repository.theme_exists("user.ocean", "1.0.0").unwrap());
    assert!(repository
        .list_component_assets(AppearanceSlot::DesktopBackground)
        .unwrap()
        .is_empty());
    assert!(paths.theme_manifest_path("user.ocean", "1.0.0").is_file());
}

#[test]
fn imports_dshwallpaper_zip_through_staging() {
    let (directory, paths, mut repository, importer) = setup();
    let folder = directory.path().join("zip-source");
    write_theme_folder(&folder, "user.zip", b"zip-background");
    let archive_path = directory.path().join("theme.dshwallpaper");
    let file = fs::File::create(&archive_path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for relative in ["theme.json", "scenes/background.webp"] {
        archive.start_file(relative, options).unwrap();
        archive
            .write_all(&fs::read(folder.join(relative)).unwrap())
            .unwrap();
    }
    archive.finish().unwrap();

    let result = importer
        .import_path(&archive_path, &mut repository)
        .unwrap();
    assert!(matches!(
        result,
        ImportResult::Theme {
            source: ImportSource::Zip,
            ..
        }
    ));
    assert!(repository.theme_exists("user.zip", "1.0.0").unwrap());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn rejects_hash_mismatch_and_leaves_no_theme_or_staging_files() {
    let (directory, paths, mut repository, importer) = setup();
    let source = directory.path().join("bad-theme");
    write_theme_folder(&source, "user.bad", b"original");
    fs::write(source.join("scenes/background.webp"), b"tampered").unwrap();

    assert!(importer.import_path(&source, &mut repository).is_err());
    assert!(!repository.theme_exists("user.bad", "1.0.0").unwrap());
    assert!(!paths.themes.join("user.bad").exists());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn rejects_zip_path_traversal() {
    let (directory, paths, mut repository, importer) = setup();
    let archive_path = directory.path().join("unsafe.zip");
    let file = fs::File::create(&archive_path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("../escape.png", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"escape").unwrap();
    archive.finish().unwrap();

    assert!(importer
        .import_path(&archive_path, &mut repository)
        .is_err());
    assert!(!directory.path().join("escape.png").exists());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn prevents_theme_assets_from_reusing_loose_asset_identity() {
    let (directory, _paths, mut repository, importer) = setup();
    let loose = directory.path().join("fat-fish.webp");
    fs::write(&loose, b"shared-content").unwrap();
    importer.import_path(&loose, &mut repository).unwrap();

    let theme = directory.path().join("theme-conflict");
    write_theme_folder(&theme, "user.conflict", b"shared-content");
    assert!(importer.import_path(&theme, &mut repository).is_err());
    assert!(!repository.theme_exists("user.conflict", "1.0.0").unwrap());
}

#[test]
fn appearance_state_imports_paths_and_returns_a_batch_snapshot() {
    let (directory, paths, repository, importer) = setup();
    let source = directory.path().join("batch.png");
    fs::write(&source, b"batch-image").unwrap();
    let state = AppearanceState::with_importer(repository, importer);

    let batch = state
        .import_paths(&[source.to_string_lossy().into_owned()])
        .unwrap();
    assert_eq!(batch.results.len(), 1);
    assert_eq!(batch.results[0].kind, "inbox");
    assert_eq!(batch.results[0].source, "file");
    assert_eq!(batch.results[0].imported, 1);
    assert!(batch.snapshot.active_theme.is_none());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn rejects_folder_symlinks_when_the_platform_can_create_them() {
    let (directory, _paths, mut repository, importer) = setup();
    let source = directory.path().join("symlink-source");
    fs::create_dir_all(&source).unwrap();
    let outside = directory.path().join("outside.png");
    fs::write(&outside, b"outside").unwrap();

    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&outside, source.join("link.png"));
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, source.join("link.png"));

    if linked.is_ok() {
        assert!(importer.import_path(&source, &mut repository).is_err());
    }
}

#[test]
fn enforces_file_count_and_size_limits() {
    let directory = tempdir().unwrap();
    let paths = AppearancePaths::new(directory.path().join("limited-app-data"));
    paths.create().unwrap();
    let mut repository = AppearanceRepository::open(&paths.catalog).unwrap();
    let importer = AppearanceImporter::with_limits(
        paths.clone(),
        ImportLimits {
            max_files: 1,
            max_file_size: 4,
            max_package_size: 4,
        },
    )
    .unwrap();
    let too_large = directory.path().join("large.png");
    fs::write(&too_large, b"12345").unwrap();
    assert!(importer.import_path(&too_large, &mut repository).is_err());

    let folder = directory.path().join("too-many");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("a.png"), b"a").unwrap();
    fs::write(folder.join("b.png"), b"b").unwrap();
    assert!(importer.import_path(&folder, &mut repository).is_err());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn rejects_default_oversized_file_before_copying_into_staging() {
    let (directory, paths, mut repository, importer) = setup();
    let too_large = directory.path().join("too-large.png");
    let file = fs::File::create(&too_large).unwrap();
    file.set_len(DEFAULT_MAX_ASSET_BYTES + 1).unwrap();

    assert!(importer.import_path(&too_large, &mut repository).is_err());
    assert!(!paths.inbox.join("too-large.png").exists());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn rejects_zip_expansion_ratio_before_writing_the_payload() {
    let (directory, paths, mut repository, importer) = setup();
    let archive_path = directory.path().join("expanded.zip");
    let file = fs::File::create(&archive_path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("background.webp", options).unwrap();
    archive.write_all(&vec![b'a'; 2 * 1024 * 1024]).unwrap();
    archive.finish().unwrap();

    assert!(importer
        .import_path(&archive_path, &mut repository)
        .is_err());
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
}

#[test]
fn rejects_theme_identity_path_traversal() {
    let (directory, paths, mut repository, importer) = setup();
    let source = directory.path().join("identity-escape");
    write_theme_folder(&source, "user.safe", b"identity");
    let manifest_path = source.join("theme.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["id"] = json!("../escape");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    assert!(importer.import_path(&source, &mut repository).is_err());
    assert!(!paths.root.parent().unwrap().join("escape").exists());
}

#[test]
fn rejects_case_insensitive_zip_path_collisions() {
    let (directory, _paths, mut repository, importer) = setup();
    let archive_path = directory.path().join("collision.zip");
    let file = fs::File::create(&archive_path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    for name in ["Fish.png", "fish.png"] {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(name.as_bytes()).unwrap();
    }
    archive.finish().unwrap();

    assert!(importer
        .import_path(&archive_path, &mut repository)
        .is_err());
}

#[test]
fn rejects_unsafe_versions_and_incompatible_component_kinds() {
    let (directory, _paths, mut repository, importer) = setup();
    let source = directory.path().join("bad-contract");
    write_theme_folder(&source, "user.contract", b"contract");
    let manifest_path = source.join("theme.json");
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    let mut unsafe_version = original.clone();
    unsafe_version["version"] = json!("..");
    fs::write(&manifest_path, serde_json::to_vec(&unsafe_version).unwrap()).unwrap();
    assert!(importer.import_path(&source, &mut repository).is_err());

    let mut wrong_kind = original;
    wrong_kind["components"] = json!({
        "wake.sequence": { "kind": "asset", "path": "scenes/background.webp" }
    });
    fs::write(&manifest_path, serde_json::to_vec(&wrong_kind).unwrap()).unwrap();
    assert!(importer.import_path(&source, &mut repository).is_err());
}
