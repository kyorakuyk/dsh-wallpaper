use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{
    ActiveTheme, AppearanceExporter, AppearanceImporter, AppearanceRepository, AppearanceSlot,
    AssetMediaType, AssetOrigin, AssetRecord, AssetStatus, ExportError, ExportResult,
    ExportThemeMetadata, ImportError, ImportResult, ImportSource, StoreError, ThemeRecord,
    ThemeSource,
};

pub struct AppearanceState {
    repository: Mutex<AppearanceRepository>,
    importer: Option<AppearanceImporter>,
    exporter: Option<AppearanceExporter>,
    paths: Option<super::AppearancePaths>,
}

impl AppearanceState {
    pub fn new(repository: AppearanceRepository) -> Self {
        Self {
            repository: Mutex::new(repository),
            importer: None,
            exporter: None,
            paths: None,
        }
    }

    pub fn with_importer(repository: AppearanceRepository, importer: AppearanceImporter) -> Self {
        Self {
            repository: Mutex::new(repository),
            importer: Some(importer),
            exporter: None,
            paths: None,
        }
    }

    pub fn with_io(
        repository: AppearanceRepository,
        importer: AppearanceImporter,
        exporter: AppearanceExporter,
    ) -> Self {
        Self {
            repository: Mutex::new(repository),
            importer: Some(importer),
            exporter: Some(exporter),
            paths: None,
        }
    }

    pub fn with_runtime_io(
        repository: AppearanceRepository,
        importer: AppearanceImporter,
        exporter: AppearanceExporter,
        paths: super::AppearancePaths,
    ) -> Self {
        Self {
            repository: Mutex::new(repository),
            importer: Some(importer),
            exporter: Some(exporter),
            paths: Some(paths),
        }
    }

    pub fn get_state(&self) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
        let repository = self.lock()?;
        snapshot(&repository)
    }

    pub fn list_themes(&self) -> Result<Vec<ThemeSummaryDto>, AppearanceCommandError> {
        Ok(self
            .lock()?
            .list_themes()?
            .into_iter()
            .map(ThemeSummaryDto::from)
            .collect())
    }

    pub fn list_assets(
        &self,
        slot: Option<&str>,
    ) -> Result<Vec<AssetSummaryDto>, AppearanceCommandError> {
        let slot = slot.map(parse_slot).transpose()?;
        Ok(self
            .lock()?
            .list_library_assets(slot)?
            .into_iter()
            .map(AssetSummaryDto::from)
            .collect())
    }

    pub fn activate_theme(
        &self,
        id: &str,
        version: &str,
    ) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
        let mut repository = self.lock()?;
        repository.activate_theme(id, version)?;
        snapshot(&repository)
    }

    pub fn set_override(
        &self,
        slot: &str,
        asset_id: &str,
    ) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
        let slot = parse_slot(slot)?;
        let mut repository = self.lock()?;
        repository.set_override(slot, asset_id)?;
        snapshot(&repository)
    }

    pub fn clear_override(
        &self,
        slot: Option<&str>,
    ) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
        let slot = slot.map(parse_slot).transpose()?;
        let mut repository = self.lock()?;
        repository.clear_override(slot)?;
        snapshot(&repository)
    }

    pub fn import_paths(&self, paths: &[String]) -> Result<ImportBatchDto, AppearanceCommandError> {
        if paths.is_empty() {
            return Err(AppearanceCommandError::invalid_argument());
        }
        let importer = self
            .importer
            .as_ref()
            .ok_or_else(AppearanceCommandError::internal)?;
        let mut repository = self.lock()?;
        let mut results = Vec::with_capacity(paths.len());
        for path in paths {
            if path.trim().is_empty() {
                return Err(AppearanceCommandError::invalid_argument());
            }
            results.push(ImportResultDto::from(
                importer.import_path(Path::new(path), &mut repository)?,
            ));
        }
        Ok(ImportBatchDto {
            results,
            snapshot: snapshot(&repository)?,
        })
    }

    pub fn classify_asset(
        &self,
        asset_id: &str,
        slots: &[String],
    ) -> Result<AssetSummaryDto, AppearanceCommandError> {
        let slots = slots
            .iter()
            .map(|slot| parse_slot(slot))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AssetSummaryDto::from(
            self.lock()?.classify_asset(asset_id, &slots)?,
        ))
    }

    pub fn export_current_theme(
        &self,
        metadata: ExportThemeMetadataDto,
        destination: &str,
    ) -> Result<ExportResult, AppearanceCommandError> {
        if destination.trim().is_empty() {
            return Err(AppearanceCommandError::invalid_argument());
        }
        let exporter = self
            .exporter
            .as_ref()
            .ok_or_else(AppearanceCommandError::internal)?;
        let metadata = ExportThemeMetadata {
            id: metadata.id,
            version: metadata.version,
            name: metadata.name,
            author: metadata.author,
            description: metadata.description,
        };
        let repository = self.lock()?;
        Ok(exporter.export_current_theme(&*repository, &metadata, Path::new(destination))?)
    }

    pub fn resolve_asset(
        &self,
        slot: &str,
    ) -> Result<Option<ResolvedAssetDto>, AppearanceCommandError> {
        let slot = parse_slot(slot)?;
        let paths = self
            .paths
            .as_ref()
            .ok_or_else(AppearanceCommandError::internal)?;
        let repository = self.lock()?;
        let Some(asset) = repository.resolve_active_asset(slot)? else {
            return Ok(None);
        };
        let source = paths.root.join(&asset.object_path);
        let bytes = std::fs::read(&source).map_err(|_| AppearanceCommandError::internal())?;
        Ok(Some(ResolvedAssetDto {
            id: asset.id,
            media_type: asset.media_type.as_str().into(),
            mime_type: mime_type(&asset.original_name).into(),
            bytes_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }))
    }

    pub fn resolve_library_asset(
        &self,
        asset_id: &str,
    ) -> Result<Option<ResolvedAssetDto>, AppearanceCommandError> {
        if asset_id.trim().is_empty() {
            return Err(AppearanceCommandError::invalid_argument());
        }
        let paths = self
            .paths
            .as_ref()
            .ok_or_else(AppearanceCommandError::internal)?;
        let repository = self.lock()?;
        let asset = repository
            .list_library_assets(None)?
            .into_iter()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| AppearanceCommandError::from(StoreError::NotFound("asset not found".into())))?;
        let source = paths.root.join(&asset.object_path);
        let bytes = std::fs::read(&source).map_err(|_| AppearanceCommandError::internal())?;
        Ok(Some(ResolvedAssetDto {
            id: asset.id,
            media_type: asset.media_type.as_str().into(),
            mime_type: mime_type(&asset.original_name).into(),
            bytes_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }))
    }

    fn lock(&self) -> Result<MutexGuard<'_, AppearanceRepository>, AppearanceCommandError> {
        self.repository
            .lock()
            .map_err(|_| AppearanceCommandError::internal())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSnapshotDto {
    pub active_theme: Option<ActiveThemeDto>,
    pub overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveThemeDto {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSummaryDto {
    pub id: String,
    pub version: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub preview: Option<String>,
    pub source: String,
    pub readonly: bool,
    pub installed_at: Option<i64>,
    pub baseline_id: String,
    pub baseline_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSummaryDto {
    pub id: String,
    pub sha256: String,
    pub media_type: String,
    pub original_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_alpha: Option<bool>,
    pub status: String,
    pub slots: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAssetDto {
    pub id: String,
    pub media_type: String,
    pub mime_type: String,
    pub bytes_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBatchDto {
    pub results: Vec<ImportResultDto>,
    pub snapshot: AppearanceSnapshotDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResultDto {
    pub kind: String,
    pub source: String,
    pub imported: usize,
    pub deduplicated: usize,
    pub theme_id: Option<String>,
    pub theme_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportThemeMetadataDto {
    pub id: String,
    pub version: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceCommandError {
    pub code: &'static str,
    pub message: &'static str,
}

impl AppearanceCommandError {
    fn invalid_argument() -> Self {
        Self {
            code: "APPEARANCE_INVALID_ARGUMENT",
            message: "外观请求参数无效",
        }
    }

    fn internal() -> Self {
        Self {
            code: "APPEARANCE_INTERNAL",
            message: "外观服务暂时不可用",
        }
    }
}

impl From<StoreError> for AppearanceCommandError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::InvalidRecord(_) => Self::invalid_argument(),
            StoreError::NotFound(_) => Self {
                code: "APPEARANCE_NOT_FOUND",
                message: "指定的主题或素材不存在",
            },
            StoreError::Database(_) => Self::internal(),
        }
    }
}

impl From<ImportError> for AppearanceCommandError {
    fn from(error: ImportError) -> Self {
        match error {
            ImportError::InvalidPackage(_) | ImportError::Json(_) | ImportError::Zip(_) => Self {
                code: "APPEARANCE_IMPORT_INVALID",
                message: "导入内容无效或不受支持",
            },
            ImportError::Io(_) => Self {
                code: "APPEARANCE_IMPORT_IO",
                message: "无法读取或保存导入内容",
            },
            ImportError::Store(error) => error.into(),
        }
    }
}

impl From<ExportError> for AppearanceCommandError {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::InvalidState(_) => Self {
                code: "APPEARANCE_EXPORT_INVALID",
                message: "当前外观无法完整导出",
            },
            ExportError::Io(_) | ExportError::Zip(_) => Self {
                code: "APPEARANCE_EXPORT_IO",
                message: "无法写入主题包",
            },
            ExportError::Json(_) => Self::internal(),
            ExportError::Store(error) => error.into(),
        }
    }
}

fn parse_slot(value: &str) -> Result<AppearanceSlot, AppearanceCommandError> {
    value
        .parse()
        .map_err(|_| AppearanceCommandError::invalid_argument())
}

fn snapshot(
    repository: &AppearanceRepository,
) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
    Ok(AppearanceSnapshotDto {
        active_theme: repository.active_theme()?.map(ActiveThemeDto::from),
        overrides: repository
            .list_overrides()?
            .into_iter()
            .map(|(slot, asset_id)| (slot.to_string(), asset_id))
            .collect(),
    })
}

fn mime_type(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

impl From<ActiveTheme> for ActiveThemeDto {
    fn from(theme: ActiveTheme) -> Self {
        Self {
            id: theme.id,
            version: theme.version,
        }
    }
}

impl From<ThemeRecord> for ThemeSummaryDto {
    fn from(theme: ThemeRecord) -> Self {
        Self {
            id: theme.id,
            version: theme.version,
            name: theme.name,
            author: theme.author,
            description: theme.description,
            preview: theme.preview,
            source: match theme.source {
                ThemeSource::Official => "official",
                ThemeSource::User => "user",
            }
            .into(),
            readonly: theme.readonly,
            installed_at: theme.installed_at,
            baseline_id: theme.baseline_id,
            baseline_version: theme.baseline_version,
        }
    }
}

impl From<AssetRecord> for AssetSummaryDto {
    fn from(asset: AssetRecord) -> Self {
        debug_assert!(matches!(asset.origin, AssetOrigin::Loose));
        Self {
            id: asset.id,
            sha256: asset.sha256,
            media_type: match asset.media_type {
                AssetMediaType::Image => "image",
                AssetMediaType::Font => "font",
                AssetMediaType::Sequence => "sequence",
                AssetMediaType::Skin => "skin",
            }
            .into(),
            original_name: asset.original_name,
            width: asset.width,
            height: asset.height,
            has_alpha: asset.has_alpha,
            status: match asset.status {
                AssetStatus::Inbox => "inbox",
                AssetStatus::Classified => "classified",
                AssetStatus::Corrupt => "corrupt",
            }
            .into(),
            slots: asset
                .slots
                .into_iter()
                .map(|slot| slot.to_string())
                .collect(),
            created_at: asset.created_at,
        }
    }
}

impl From<ImportResult> for ImportResultDto {
    fn from(result: ImportResult) -> Self {
        let source_name = |source: ImportSource| match source {
            ImportSource::Folder => "folder",
            ImportSource::Zip => "archive",
            ImportSource::File => "file",
        };
        match result {
            ImportResult::Inbox {
                source,
                imported,
                deduplicated,
            } => Self {
                kind: "inbox".into(),
                source: source_name(source).into(),
                imported,
                deduplicated,
                theme_id: None,
                theme_version: None,
            },
            ImportResult::Theme {
                source,
                id,
                version,
                imported,
                deduplicated,
            } => Self {
                kind: "theme".into(),
                source: source_name(source).into(),
                imported,
                deduplicated,
                theme_id: Some(id),
                theme_version: Some(version),
            },
        }
    }
}

#[tauri::command]
pub(crate) fn appearance_get_state(
    state: State<'_, AppearanceState>,
) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
    state.get_state()
}

#[tauri::command]
pub(crate) fn appearance_list_themes(
    state: State<'_, AppearanceState>,
) -> Result<Vec<ThemeSummaryDto>, AppearanceCommandError> {
    state.list_themes()
}

#[tauri::command]
pub(crate) fn appearance_list_assets(
    state: State<'_, AppearanceState>,
    slot: Option<String>,
) -> Result<Vec<AssetSummaryDto>, AppearanceCommandError> {
    state.list_assets(slot.as_deref())
}

#[tauri::command]
pub(crate) fn appearance_activate_theme(
    state: State<'_, AppearanceState>,
    id: String,
    version: String,
) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
    state.activate_theme(&id, &version)
}

#[tauri::command]
pub(crate) fn appearance_set_override(
    state: State<'_, AppearanceState>,
    slot: String,
    asset_id: String,
) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
    state.set_override(&slot, &asset_id)
}

#[tauri::command]
pub(crate) fn appearance_clear_override(
    state: State<'_, AppearanceState>,
    slot: Option<String>,
) -> Result<AppearanceSnapshotDto, AppearanceCommandError> {
    state.clear_override(slot.as_deref())
}

#[tauri::command]
pub(crate) fn appearance_import_paths(
    state: State<'_, AppearanceState>,
    paths: Vec<String>,
) -> Result<ImportBatchDto, AppearanceCommandError> {
    state.import_paths(&paths)
}

#[tauri::command]
pub(crate) fn appearance_classify_asset(
    state: State<'_, AppearanceState>,
    asset_id: String,
    slots: Vec<String>,
) -> Result<AssetSummaryDto, AppearanceCommandError> {
    state.classify_asset(&asset_id, &slots)
}

#[tauri::command]
pub(crate) fn appearance_export_current_theme(
    state: State<'_, AppearanceState>,
    metadata: ExportThemeMetadataDto,
    destination: String,
) -> Result<ExportResult, AppearanceCommandError> {
    state.export_current_theme(metadata, &destination)
}

#[tauri::command]
pub(crate) fn appearance_resolve_asset(
    state: State<'_, AppearanceState>,
    slot: String,
) -> Result<Option<ResolvedAssetDto>, AppearanceCommandError> {
    state.resolve_asset(&slot)
}

#[tauri::command]
pub(crate) fn appearance_resolve_library_asset(
    state: State<'_, AppearanceState>,
    asset_id: String,
) -> Result<Option<ResolvedAssetDto>, AppearanceCommandError> {
    state.resolve_library_asset(&asset_id)
}
