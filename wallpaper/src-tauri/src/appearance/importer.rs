use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use super::paths::AppearancePaths;
use super::repository::{AppearanceRepository, StoreError};
use super::types::{
    AppearanceSlot, AssetMediaType, AssetOrigin, AssetRecord, AssetStatus, ThemeAssetLink,
    ThemeFileRecord, ThemeRecord, ThemeSource,
};

static BATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Appearance assets are rendered into a WebView and later crossed through
/// an IPC/base64 boundary. Keep the default below a size that can create a
/// multi-hundred-megabyte transient allocation in the renderer.
pub const DEFAULT_MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ZIP_EXPANSION_RATIO: u64 = 100;

#[derive(Debug, Clone, Copy)]
pub struct ImportLimits {
    pub max_files: usize,
    pub max_file_size: u64,
    pub max_package_size: u64,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_files: 256,
            max_file_size: DEFAULT_MAX_ASSET_BYTES,
            max_package_size: 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub enum ImportError {
    Io(io::Error),
    Json(serde_json::Error),
    Zip(zip::result::ZipError),
    Store(StoreError),
    InvalidPackage(String),
}

impl From<io::Error> for ImportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for ImportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<zip::result::ZipError> for ImportError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}
impl From<StoreError> for ImportError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    Folder,
    Zip,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportResult {
    Inbox {
        source: ImportSource,
        imported: usize,
        deduplicated: usize,
    },
    Theme {
        source: ImportSource,
        id: String,
        version: String,
        imported: usize,
        deduplicated: usize,
    },
}

#[derive(Debug, Clone)]
struct StagedFile {
    path: String,
    source: PathBuf,
    size: u64,
    sha256: String,
}

pub struct AppearanceImporter {
    paths: AppearancePaths,
    limits: ImportLimits,
}

impl AppearanceImporter {
    pub fn new(paths: AppearancePaths) -> Result<Self, ImportError> {
        paths.create()?;
        Ok(Self {
            paths,
            limits: ImportLimits::default(),
        })
    }

    pub fn with_limits(paths: AppearancePaths, limits: ImportLimits) -> Result<Self, ImportError> {
        if limits.max_files == 0 || limits.max_file_size == 0 || limits.max_package_size == 0 {
            return Err(ImportError::InvalidPackage("导入限制必须大于零".into()));
        }
        paths.create()?;
        Ok(Self { paths, limits })
    }

    pub fn import_path(
        &self,
        source: &Path,
        repository: &mut AppearanceRepository,
    ) -> Result<ImportResult, ImportError> {
        let source_kind = source_kind(source)?;
        let batch = unique_batch_id();
        let staging = self.paths.staging.join(&batch);
        fs::create_dir_all(&staging)?;
        let result = self.import_into_staging(source, source_kind.clone(), &staging, repository);
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn import_into_staging(
        &self,
        source: &Path,
        source_kind: ImportSource,
        staging: &Path,
        repository: &mut AppearanceRepository,
    ) -> Result<ImportResult, ImportError> {
        let files = match source_kind {
            ImportSource::Zip => stage_zip(source, staging, self.limits)?,
            ImportSource::Folder => stage_folder(source, staging, self.limits)?,
            ImportSource::File => stage_single_file(source, staging, self.limits)?,
        };
        if files.is_empty() {
            return Err(ImportError::InvalidPackage("导入内容为空".into()));
        }
        let manifest_file = files.iter().find(|file| file.path == "theme.json");
        if let Some(manifest_file) = manifest_file {
            let mut content = String::new();
            File::open(&manifest_file.source)?.read_to_string(&mut content)?;
            let manifest: Value = serde_json::from_str(&content)?;
            self.install_theme(manifest, files, &source_kind, staging, repository)
        } else {
            self.install_inbox(files, source_kind, repository)
        }
    }

    fn install_inbox(
        &self,
        files: Vec<StagedFile>,
        source: ImportSource,
        repository: &mut AppearanceRepository,
    ) -> Result<ImportResult, ImportError> {
        let mut imported = 0;
        let mut deduplicated = 0;
        for file in files {
            let (media_type, extension) = media_type_for_path(&file.path)?;
            let id = format!("asset-{}", &file.sha256[..16]);
            let asset = AssetRecord {
                id,
                sha256: file.sha256.clone(),
                media_type,
                original_name: Path::new(&file.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("imported-asset")
                    .to_owned(),
                object_path: object_relative_path(&file.sha256, &extension),
                width: None,
                height: None,
                has_alpha: None,
                status: AssetStatus::Inbox,
                slots: Vec::new(),
                origin: AssetOrigin::Loose,
                created_at: unix_timestamp(),
            };
            if repository.asset_by_sha256(&asset.sha256)?.is_some() {
                deduplicated += 1;
                continue;
            }
            copy_object(&self.paths, &file.source, &file.sha256, &extension)?;
            repository.insert_asset(&asset)?;
            imported += 1;
        }
        Ok(ImportResult::Inbox {
            source,
            imported,
            deduplicated,
        })
    }

    fn install_theme(
        &self,
        manifest: Value,
        files: Vec<StagedFile>,
        source: &ImportSource,
        staging: &Path,
        repository: &mut AppearanceRepository,
    ) -> Result<ImportResult, ImportError> {
        let parsed = parse_theme_manifest(&manifest)?;
        validate_theme_inventory(&manifest, &files)?;
        if repository.theme_exists(&parsed.theme.id, &parsed.theme.version)? {
            return Err(ImportError::InvalidPackage("主题版本已经安装".into()));
        }

        let by_path: BTreeMap<_, _> = files.iter().map(|file| (file.path.clone(), file)).collect();
        let mut assets = Vec::new();
        let mut links = Vec::new();
        let mut theme_files = Vec::new();
        let mut deduplicated = 0;
        let mut imported = 0;
        let mut asset_by_path = BTreeMap::<String, String>::new();
        let mut asset_by_hash = BTreeMap::<String, String>::new();

        for entry in parsed.files {
            let file = by_path.get(&entry.path).ok_or_else(|| {
                ImportError::InvalidPackage(format!("缺少主题文件：{}", entry.path))
            })?;
            let (media_type, extension) = media_type_for_path(&file.path)?;
            let generated_asset_id = format!("asset-{}", &file.sha256[..16]);
            let asset_id = if let Some(asset_id) = asset_by_hash.get(&file.sha256) {
                deduplicated += 1;
                asset_id.clone()
            } else if repository.asset_by_sha256(&file.sha256)?.is_some() {
                return Err(ImportError::InvalidPackage(format!(
                    "主题资源与已有素材重复，无法保持主题私有隔离：{}",
                    file.path
                )));
            } else {
                copy_object(&self.paths, &file.source, &file.sha256, &extension)?;
                assets.push(AssetRecord {
                    id: generated_asset_id.clone(),
                    sha256: file.sha256.clone(),
                    media_type,
                    original_name: Path::new(&file.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("theme-asset")
                        .to_owned(),
                    object_path: object_relative_path(&file.sha256, &extension),
                    width: None,
                    height: None,
                    has_alpha: None,
                    status: AssetStatus::Classified,
                    slots: Vec::new(),
                    origin: AssetOrigin::ThemePrivate {
                        theme_id: parsed.theme.id.clone(),
                        theme_version: parsed.theme.version.clone(),
                    },
                    created_at: unix_timestamp(),
                });
                imported += 1;
                generated_asset_id
            };
            asset_by_hash.insert(file.sha256.clone(), asset_id.clone());
            asset_by_path.insert(file.path.clone(), asset_id.clone());
            theme_files.push(ThemeFileRecord {
                theme_id: parsed.theme.id.clone(),
                theme_version: parsed.theme.version.clone(),
                package_path: file.path.clone(),
                sha256: file.sha256.clone(),
                size: file.size,
                asset_id: Some(asset_id),
            });
        }

        let referenced = component_references(&manifest)?;
        for (slot, paths) in referenced {
            for path in paths {
                let asset_id = asset_by_path.get(&path).ok_or_else(|| {
                    ImportError::InvalidPackage(format!("组件引用未列入 files：{path}"))
                })?;
                links.push(ThemeAssetLink {
                    theme_id: parsed.theme.id.clone(),
                    theme_version: parsed.theme.version.clone(),
                    slot,
                    package_path: path,
                    asset_id: asset_id.clone(),
                });
                if let Some(asset) = assets.iter_mut().find(|asset| &asset.id == asset_id) {
                    if !asset.slots.contains(&slot) {
                        asset.slots.push(slot);
                    }
                }
            }
        }

        let theme_directory = self
            .paths
            .themes
            .join(&parsed.theme.id)
            .join(&parsed.theme.version);
        copy_theme_files(staging, &theme_directory, &files)?;
        if let Err(error) =
            repository.install_verified_theme(&parsed.theme, &assets, &theme_files, &links)
        {
            let _ = fs::remove_dir_all(&theme_directory);
            return Err(error.into());
        }
        Ok(ImportResult::Theme {
            source: source.clone(),
            id: parsed.theme.id,
            version: parsed.theme.version,
            imported,
            deduplicated,
        })
    }
}

struct ParsedTheme {
    theme: ThemeRecord,
    files: Vec<ManifestFile>,
}

struct ManifestFile {
    path: String,
    sha256: String,
    size: u64,
}

fn source_kind(path: &Path) -> Result<ImportSource, ImportError> {
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        Ok(ImportSource::Folder)
    } else if metadata.is_file() {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        if extension.eq_ignore_ascii_case("zip") || extension.eq_ignore_ascii_case("dshwallpaper") {
            Ok(ImportSource::Zip)
        } else {
            Ok(ImportSource::File)
        }
    } else {
        Err(ImportError::InvalidPackage(
            "导入路径不是普通文件或文件夹".into(),
        ))
    }
}

fn stage_single_file(
    source: &Path,
    staging: &Path,
    limits: ImportLimits,
) -> Result<Vec<StagedFile>, ImportError> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ImportError::InvalidPackage("文件名无效".into()))?;
    let destination = staging.join(name);
    let metadata = fs::metadata(source)?;
    if !metadata.is_file() {
        return Err(ImportError::InvalidPackage("导入路径不是普通文件".into()));
    }
    if metadata.len() > limits.max_file_size {
        return Err(ImportError::InvalidPackage("主题包单文件过大".into()));
    }
    fs::copy(source, &destination)?;
    let file = hash_file(&destination, name.into(), limits)?;
    enforce_package_limits(&[file], limits)
}

fn stage_folder(
    source: &Path,
    staging: &Path,
    limits: ImportLimits,
) -> Result<Vec<StagedFile>, ImportError> {
    let mut files = Vec::new();
    collect_folder_files(source, source, staging, &mut files, limits)?;
    enforce_package_limits(&files, limits)
}

fn collect_folder_files(
    root: &Path,
    current: &Path,
    staging: &Path,
    files: &mut Vec<StagedFile>,
    limits: ImportLimits,
) -> Result<(), ImportError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.file_type().is_symlink() {
            return Err(ImportError::InvalidPackage("主题包不允许符号链接".into()));
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ImportError::InvalidPackage("文件路径无效".into()))?;
        let package_path = normalize_relative_path(relative)?;
        if metadata.is_dir() {
            collect_folder_files(root, &path, staging, files, limits)?;
        } else if metadata.is_file() {
            if metadata.len() > limits.max_file_size {
                return Err(ImportError::InvalidPackage("主题包单文件过大".into()));
            }
            let destination = staging.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &destination)?;
            files.push(hash_file(&destination, package_path, limits)?);
            enforce_package_limits(files, limits)?;
        }
    }
    Ok(())
}

fn stage_zip(
    source: &Path,
    staging: &Path,
    limits: ImportLimits,
) -> Result<Vec<StagedFile>, ImportError> {
    let file = File::open(source)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > limits.max_files {
        return Err(ImportError::InvalidPackage("主题包文件数量超限".into()));
    }
    let mut files = Vec::new();
    let mut total_size = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        let path = normalize_relative_path(Path::new(&name))?;
        if entry.is_dir() {
            continue;
        }
        let declared_size = entry.size();
        let compressed_size = entry.compressed_size();
        if declared_size > limits.max_file_size {
            return Err(ImportError::InvalidPackage("主题包单文件过大".into()));
        }
        // Reject highly expanded entries before writing them to staging. The
        // declared size is checked again while reading because ZIP metadata
        // is untrusted and may not match the bytes actually produced.
        if declared_size > compressed_size.saturating_mul(MAX_ZIP_EXPANSION_RATIO) {
            return Err(ImportError::InvalidPackage("主题包压缩倍率超限".into()));
        }
        if entry
            .unix_mode()
            .map(|mode| mode & 0o170000 == 0o120000)
            .unwrap_or(false)
        {
            return Err(ImportError::InvalidPackage("主题包不允许符号链接".into()));
        }
        let destination = staging.join(&path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&destination)?;
        let mut size = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            size = size.saturating_add(read as u64);
            if size > limits.max_file_size {
                return Err(ImportError::InvalidPackage("主题包单文件过大".into()));
            }
            total_size = total_size.saturating_add(read as u64);
            if total_size > limits.max_package_size {
                return Err(ImportError::InvalidPackage("主题包总大小超限".into()));
            }
            output.write_all(&buffer[..read])?;
        }
        output.flush()?;
        files.push(hash_file(&destination, path, limits)?);
    }
    enforce_package_limits(&files, limits)
}

fn enforce_package_limits(
    files: &[StagedFile],
    limits: ImportLimits,
) -> Result<Vec<StagedFile>, ImportError> {
    if files.len() > limits.max_files {
        return Err(ImportError::InvalidPackage("主题包文件数量超限".into()));
    }
    let mut canonical_paths = BTreeSet::new();
    for file in files {
        if !canonical_paths.insert(file.path.to_ascii_lowercase()) {
            return Err(ImportError::InvalidPackage(
                "主题包包含 Windows 下冲突的重复路径".into(),
            ));
        }
    }
    let total = files
        .iter()
        .try_fold(0u64, |total, file| total.checked_add(file.size))
        .ok_or_else(|| ImportError::InvalidPackage("主题包大小溢出".into()))?;
    if total > limits.max_package_size {
        return Err(ImportError::InvalidPackage("主题包总大小超限".into()));
    }
    Ok(files.to_vec())
}

fn hash_file(
    path: &Path,
    package_path: String,
    limits: ImportLimits,
) -> Result<StagedFile, ImportError> {
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > limits.max_file_size {
            return Err(ImportError::InvalidPackage("主题包单文件过大".into()));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(StagedFile {
        path: package_path,
        source: path.to_owned(),
        size,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn normalize_relative_path(path: &Path) -> Result<String, ImportError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| ImportError::InvalidPackage("文件名不是有效 UTF-8".into()))?;
                if value.is_empty()
                    || value.ends_with('.')
                    || value.ends_with(' ')
                    || value.contains(':')
                    || is_windows_device_name(value)
                {
                    return Err(ImportError::InvalidPackage("主题包包含不安全路径".into()));
                }
                parts.push(value.replace('\\', "/"));
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ImportError::InvalidPackage(
                    "主题包包含路径穿越或绝对路径".into(),
                ))
            }
        }
    }
    if parts.is_empty() {
        return Err(ImportError::InvalidPackage("主题包包含空路径".into()));
    }
    Ok(parts.join("/"))
}

fn is_windows_device_name(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value);
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn media_type_for_path(path: &str) -> Result<(AssetMediaType, String), ImportError> {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media = match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => AssetMediaType::Image,
        "ttf" | "otf" | "woff2" => AssetMediaType::Font,
        "json" => AssetMediaType::Skin,
        _ => {
            return Err(ImportError::InvalidPackage(format!(
                "不支持的素材类型：{path}"
            )))
        }
    };
    Ok((media, extension))
}

fn object_relative_path(sha256: &str, extension: &str) -> String {
    format!(
        "library/objects/sha256/{}/{sha256}.{extension}",
        &sha256[..2]
    )
}

fn copy_object(
    paths: &AppearancePaths,
    source: &Path,
    sha256: &str,
    extension: &str,
) -> Result<(), ImportError> {
    let destination = paths
        .object_path(sha256, extension)
        .ok_or_else(|| ImportError::InvalidPackage("素材哈希或扩展名无效".into()))?;
    if destination.exists() {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn copy_theme_files(
    staging: &Path,
    destination: &Path,
    files: &[StagedFile],
) -> Result<(), ImportError> {
    fs::create_dir_all(destination)?;
    for file in files {
        let target = destination.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(staging.join(&file.path), target)?;
    }
    Ok(())
}

fn parse_theme_manifest(value: &Value) -> Result<ParsedTheme, ImportError> {
    let object = value
        .as_object()
        .ok_or_else(|| ImportError::InvalidPackage("theme.json 必须是对象".into()))?;
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || object.get("kind").and_then(Value::as_str) != Some("theme")
    {
        return Err(ImportError::InvalidPackage("主题 schema 不受支持".into()));
    }
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ImportError::InvalidPackage(format!("主题字段无效：{key}")))
    };
    let id = text("id")?;
    let version = text("version")?;
    let name = text("name")?;
    if !is_safe_identifier(&id) || !is_safe_version(&version) {
        return Err(ImportError::InvalidPackage(
            "主题 id 或 version 格式无效".into(),
        ));
    }
    let compatibility = object
        .get("compatibility")
        .and_then(Value::as_object)
        .ok_or_else(|| ImportError::InvalidPackage("compatibility 无效".into()))?;
    let _min_app_version = compatibility
        .get("minAppVersion")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ImportError::InvalidPackage("compatibility.minAppVersion 无效".into()))?;
    let baseline = object
        .get("baseline")
        .and_then(Value::as_object)
        .ok_or_else(|| ImportError::InvalidPackage("baseline 无效".into()))?;
    let baseline_id = baseline
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ImportError::InvalidPackage("baseline.id 无效".into()))?;
    let baseline_version = baseline
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ImportError::InvalidPackage("baseline.version 无效".into()))?;
    if !is_safe_identifier(baseline_id) || !is_safe_version(baseline_version) {
        return Err(ImportError::InvalidPackage(
            "baseline id 或 version 格式无效".into(),
        ));
    }
    let files = parse_manifest_files(
        object
            .get("files")
            .ok_or_else(|| ImportError::InvalidPackage("files 无效".into()))?,
    )?;
    let manifest_path = format!("library/themes/{id}/{version}/theme.json");
    Ok(ParsedTheme {
        theme: ThemeRecord {
            id: id.clone(),
            version: version.clone(),
            name,
            author: object
                .get("author")
                .and_then(Value::as_str)
                .map(str::to_owned),
            description: object
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            preview: object
                .get("preview")
                .and_then(Value::as_str)
                .map(str::to_owned),
            source: ThemeSource::User,
            manifest_path,
            readonly: false,
            installed_at: Some(unix_timestamp()),
            baseline_id: baseline_id.to_owned(),
            baseline_version: baseline_version.to_owned(),
        },
        files,
    })
}

fn parse_manifest_files(value: &Value) -> Result<Vec<ManifestFile>, ImportError> {
    let list = value
        .as_array()
        .ok_or_else(|| ImportError::InvalidPackage("files 必须是数组".into()))?;
    let mut paths = BTreeSet::new();
    let mut files = Vec::new();
    for entry in list {
        let object = entry
            .as_object()
            .ok_or_else(|| ImportError::InvalidPackage("files 条目无效".into()))?;
        let path = normalize_relative_path(Path::new(
            object
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| ImportError::InvalidPackage("文件路径无效".into()))?,
        ))?;
        if !paths.insert(path.clone()) {
            return Err(ImportError::InvalidPackage("files 存在重复路径".into()));
        }
        let sha256 = object
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| ImportError::InvalidPackage("文件哈希无效".into()))?
            .to_ascii_lowercase();
        let size = object
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| ImportError::InvalidPackage("文件大小无效".into()))?;
        files.push(ManifestFile { path, sha256, size });
    }
    Ok(files)
}

fn component_references(
    value: &Value,
) -> Result<BTreeMap<AppearanceSlot, Vec<String>>, ImportError> {
    let mut references = BTreeMap::new();
    let Some(components) = value.get("components").and_then(Value::as_object) else {
        return Ok(references);
    };
    for (slot_name, component) in components {
        let slot = slot_name
            .parse()
            .map_err(|_| ImportError::InvalidPackage(format!("未知外观槽位：{slot_name}")))?;
        let object = component
            .as_object()
            .ok_or_else(|| ImportError::InvalidPackage("组件声明无效".into()))?;
        let mut paths = Vec::new();
        match object.get("kind").and_then(Value::as_str) {
            Some("asset")
                if !matches!(
                    slot,
                    AppearanceSlot::WakeSequence | AppearanceSlot::ChatSkin
                ) =>
            {
                paths.push(
                    object
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ImportError::InvalidPackage("组件路径无效".into()))?
                        .to_owned(),
                )
            }
            Some("sequence") => {
                if slot != AppearanceSlot::WakeSequence {
                    return Err(ImportError::InvalidPackage(
                        "sequence 只能用于 wake.sequence".into(),
                    ));
                }
                let frames = object
                    .get("frames")
                    .and_then(Value::as_array)
                    .filter(|frames| !frames.is_empty())
                    .ok_or_else(|| ImportError::InvalidPackage("序列帧无效".into()))?;
                for frame in frames {
                    let frame_object = frame
                        .as_object()
                        .ok_or_else(|| ImportError::InvalidPackage("序列帧无效".into()))?;
                    let duration = frame_object
                        .get("durationMs")
                        .and_then(Value::as_u64)
                        .filter(|duration| *duration > 0)
                        .ok_or_else(|| ImportError::InvalidPackage("序列帧时长无效".into()))?;
                    let fade = frame_object
                        .get("fadeMs")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if fade > duration {
                        return Err(ImportError::InvalidPackage(
                            "序列帧淡入时长不能超过帧时长".into(),
                        ));
                    }
                    paths.push(
                        frame_object
                            .get("path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| ImportError::InvalidPackage("序列帧路径无效".into()))?
                            .to_owned(),
                    );
                }
            }
            Some("skin") => {
                if slot != AppearanceSlot::ChatSkin {
                    return Err(ImportError::InvalidPackage(
                        "skin 只能用于 chat.skin".into(),
                    ));
                }
                paths.push(
                    object
                        .get("definition")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ImportError::InvalidPackage("皮肤定义无效".into()))?
                        .to_owned(),
                );
                if let Some(textures) = object.get("textures").and_then(Value::as_array) {
                    for texture in textures {
                        paths.push(
                            texture
                                .as_str()
                                .ok_or_else(|| {
                                    ImportError::InvalidPackage("皮肤纹理路径无效".into())
                                })?
                                .to_owned(),
                        );
                    }
                }
            }
            _ => {
                return Err(ImportError::InvalidPackage(
                    "组件 kind 与外观槽位不兼容".into(),
                ))
            }
        }
        references.insert(slot, paths);
    }
    Ok(references)
}

fn validate_theme_inventory(manifest: &Value, files: &[StagedFile]) -> Result<(), ImportError> {
    let parsed = parse_manifest_files(
        manifest
            .get("files")
            .ok_or_else(|| ImportError::InvalidPackage("files 无效".into()))?,
    )?;
    let actual: BTreeMap<_, _> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for entry in &parsed {
        let actual_file = actual.get(entry.path.as_str()).ok_or_else(|| {
            ImportError::InvalidPackage(format!("主题包缺少文件：{}", entry.path))
        })?;
        if actual_file.sha256 != entry.sha256 || actual_file.size != entry.size {
            return Err(ImportError::InvalidPackage(format!(
                "主题文件校验失败：{}",
                entry.path
            )));
        }
    }
    for file in files {
        if file.path != "theme.json" && !parsed.iter().any(|entry| entry.path == file.path) {
            return Err(ImportError::InvalidPackage(format!(
                "主题包包含未声明文件：{}",
                file.path
            )));
        }
    }
    let references = component_references(manifest)?;
    for path in references.values().flatten() {
        if !parsed.iter().any(|entry| entry.path == *path) {
            return Err(ImportError::InvalidPackage(format!(
                "组件引用未列入 files：{path}"
            )));
        }
    }
    for path in auxiliary_references(manifest)? {
        if !parsed.iter().any(|entry| entry.path == path) {
            return Err(ImportError::InvalidPackage(format!(
                "主题资源引用未列入 files：{path}"
            )));
        }
    }
    Ok(())
}

fn auxiliary_references(manifest: &Value) -> Result<Vec<String>, ImportError> {
    let mut paths = Vec::new();
    if let Some(preview) = manifest.get("preview") {
        paths.push(
            preview
                .as_str()
                .ok_or_else(|| ImportError::InvalidPackage("preview 路径无效".into()))?
                .to_owned(),
        );
    }
    if let Some(ui) = manifest.get("ui") {
        let ui = ui
            .as_object()
            .ok_or_else(|| ImportError::InvalidPackage("ui 声明无效".into()))?;
        for field in ["tokens", "text", "layout"] {
            if let Some(value) = ui.get(field) {
                paths.push(
                    value
                        .as_str()
                        .ok_or_else(|| ImportError::InvalidPackage("ui 资源路径无效".into()))?
                        .to_owned(),
                );
            }
        }
    }
    for path in &paths {
        normalize_relative_path(Path::new(path))?;
    }
    Ok(paths)
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn is_safe_identifier(value: &str) -> bool {
    value.len() <= 64
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
        && !value.ends_with(['.', '_', '-'])
}

fn is_safe_version(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 || value.contains(['/', '\\']) {
        return false;
    }
    let core_end = value.find(['-', '+']).unwrap_or(value.len());
    let core = &value[..core_end];
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && value[core_end..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn unique_batch_id() -> String {
    format!(
        "{}-{}-{}",
        unix_timestamp(),
        std::process::id(),
        BATCH_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
