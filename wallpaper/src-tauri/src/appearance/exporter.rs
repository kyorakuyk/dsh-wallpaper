use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use super::{
    AppearancePaths, AppearanceRepository, AppearanceSlot, AssetMediaType, AssetRecord, StoreError,
    ThemeRecord,
};

#[derive(Debug)]
pub enum ExportError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Zip(zip::result::ZipError),
    Store(StoreError),
    InvalidState(String),
}

impl From<std::io::Error> for ExportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for ExportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<zip::result::ZipError> for ExportError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}
impl From<StoreError> for ExportError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportThemeMetadata {
    pub id: String,
    pub version: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub files: usize,
    pub bytes: u64,
}

struct ResolvedComponent {
    definition: Value,
    files: Vec<(String, PathBuf)>,
}

pub struct AppearanceExporter {
    paths: AppearancePaths,
}

impl AppearanceExporter {
    pub fn new(paths: AppearancePaths) -> Result<Self, ExportError> {
        paths.create()?;
        Ok(Self { paths })
    }

    pub fn export_current_theme(
        &self,
        repository: &AppearanceRepository,
        metadata: &ExportThemeMetadata,
        destination: &Path,
    ) -> Result<ExportResult, ExportError> {
        validate_metadata(metadata)?;
        let active = repository
            .active_theme()?
            .ok_or_else(|| ExportError::InvalidState("没有活动主题".into()))?;
        let active_theme = repository.theme_by_id_version(&active.id, &active.version)?;
        let overrides: BTreeMap<_, _> = repository.list_overrides()?.into_iter().collect();

        let mut components = Map::new();
        let mut output_files = Vec::<(String, PathBuf)>::new();
        for slot in AppearanceSlot::ALL {
            let resolved = if let Some(asset_id) = overrides.get(&slot) {
                self.resolve_override(repository.asset_by_id(asset_id)?, slot)?
            } else {
                self.resolve_theme_slot(repository, &active_theme, slot, &mut BTreeSet::new())?
            };
            components.insert(slot.to_string(), resolved.definition);
            output_files.extend(resolved.files);
        }

        let mut canonical = BTreeSet::new();
        for (path, _) in &output_files {
            if !canonical.insert(path.to_ascii_lowercase()) {
                return Err(ExportError::InvalidState(format!(
                    "导出文件路径冲突：{path}"
                )));
            }
        }

        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = destination.with_extension("dshwallpaper.tmp");
        let mut manifest_files = Vec::new();
        let mut total_bytes = 0u64;
        for (path, source) in &output_files {
            let (hash, size) = hash_file(source)?;
            total_bytes += size;
            manifest_files.push(json!({ "path": path, "sha256": hash, "size": size }));
        }
        let manifest = json!({
            "schemaVersion": 1,
            "kind": "theme",
            "id": metadata.id,
            "version": metadata.version,
            "name": metadata.name,
            "author": metadata.author,
            "description": metadata.description,
            "compatibility": { "minAppVersion": "0.3.0" },
            "baseline": { "id": active.id, "version": active.version },
            "components": components,
            "files": manifest_files
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;

        let file = File::create(&temporary)?;
        let mut archive = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("theme.json", options)?;
        archive.write_all(&manifest_bytes)?;
        for (path, source) in &output_files {
            archive.start_file(path, options)?;
            let mut input = File::open(source)?;
            std::io::copy(&mut input, &mut archive)?;
        }
        archive.finish()?.sync_all()?;
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::rename(&temporary, destination)?;

        Ok(ExportResult {
            path: destination.to_string_lossy().into_owned(),
            files: output_files.len(),
            bytes: total_bytes,
        })
    }

    fn resolve_override(
        &self,
        asset: AssetRecord,
        slot: AppearanceSlot,
    ) -> Result<ResolvedComponent, ExportError> {
        let extension = Path::new(&asset.object_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .ok_or_else(|| ExportError::InvalidState("素材扩展名无效".into()))?;
        let output = format!("components/{}/override.{extension}", slot_path(slot));
        let source = self.paths.root.join(&asset.object_path);
        if !source.is_file() {
            return Err(ExportError::InvalidState("局部覆盖素材文件缺失".into()));
        }
        match asset.media_type {
            AssetMediaType::Image | AssetMediaType::Font => Ok(ResolvedComponent {
                definition: json!({ "kind": "asset", "path": output }),
                files: vec![(output, source)],
            }),
            AssetMediaType::Sequence | AssetMediaType::Skin => Err(ExportError::InvalidState(
                "独立序列和皮肤导出需要组件包元数据".into(),
            )),
        }
    }

    fn resolve_theme_slot(
        &self,
        repository: &AppearanceRepository,
        theme: &ThemeRecord,
        slot: AppearanceSlot,
        visited: &mut BTreeSet<(String, String)>,
    ) -> Result<ResolvedComponent, ExportError> {
        if !visited.insert((theme.id.clone(), theme.version.clone())) {
            return Err(ExportError::InvalidState("主题基线存在循环继承".into()));
        }
        let manifest_path = self.resolve_manifest_path(theme);
        let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        if let Some(component) = manifest
            .get("components")
            .and_then(Value::as_object)
            .and_then(|components| components.get(slot.as_str()))
        {
            return self.copy_component(theme, slot, component.clone(), &manifest_path);
        }
        let baseline =
            repository.theme_by_id_version(&theme.baseline_id, &theme.baseline_version)?;
        self.resolve_theme_slot(repository, &baseline, slot, visited)
    }

    fn resolve_manifest_path(&self, theme: &ThemeRecord) -> PathBuf {
        let path = PathBuf::from(&theme.manifest_path);
        if path.is_absolute() {
            path
        } else {
            self.paths.root.join(path)
        }
    }

    fn copy_component(
        &self,
        theme: &ThemeRecord,
        slot: AppearanceSlot,
        mut component: Value,
        manifest_path: &Path,
    ) -> Result<ResolvedComponent, ExportError> {
        let base = manifest_path
            .parent()
            .ok_or_else(|| ExportError::InvalidState("主题 manifest 路径无效".into()))?;
        let mut files = Vec::new();
        let mut index = 0usize;
        rewrite_component_paths(&mut component, |original| {
            let source = base.join(original);
            if !source.is_file() {
                return Err(ExportError::InvalidState(format!(
                    "主题资源文件缺失：{} {} {original}",
                    theme.id, theme.version
                )));
            }
            let extension = Path::new(original)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("bin");
            let output = format!("components/{}/{index:03}.{extension}", slot_path(slot));
            index += 1;
            files.push((output.clone(), source));
            Ok(output)
        })?;
        Ok(ResolvedComponent {
            definition: component,
            files,
        })
    }
}

fn rewrite_component_paths<F>(component: &mut Value, mut rewrite: F) -> Result<(), ExportError>
where
    F: FnMut(&str) -> Result<String, ExportError>,
{
    let object = component
        .as_object_mut()
        .ok_or_else(|| ExportError::InvalidState("组件声明无效".into()))?;
    match object.get("kind").and_then(Value::as_str) {
        Some("asset") => rewrite_field(object, "path", &mut rewrite),
        Some("sequence") => {
            let frames = object
                .get_mut("frames")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| ExportError::InvalidState("序列声明无效".into()))?;
            for frame in frames {
                rewrite_field(
                    frame
                        .as_object_mut()
                        .ok_or_else(|| ExportError::InvalidState("序列帧无效".into()))?,
                    "path",
                    &mut rewrite,
                )?;
            }
            Ok(())
        }
        Some("skin") => {
            rewrite_field(object, "definition", &mut rewrite)?;
            if let Some(textures) = object.get_mut("textures").and_then(Value::as_array_mut) {
                for texture in textures {
                    let original = texture
                        .as_str()
                        .ok_or_else(|| ExportError::InvalidState("皮肤纹理无效".into()))?;
                    *texture = Value::String(rewrite(original)?);
                }
            }
            Ok(())
        }
        _ => Err(ExportError::InvalidState("组件 kind 无效".into())),
    }
}

fn rewrite_field<F>(
    object: &mut Map<String, Value>,
    field: &str,
    rewrite: &mut F,
) -> Result<(), ExportError>
where
    F: FnMut(&str) -> Result<String, ExportError>,
{
    let original = object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ExportError::InvalidState("组件资源路径无效".into()))?;
    object.insert(field.into(), Value::String(rewrite(original)?));
    Ok(())
}

fn validate_metadata(metadata: &ExportThemeMetadata) -> Result<(), ExportError> {
    if !is_safe_identifier(&metadata.id)
        || !is_safe_version(&metadata.version)
        || metadata.name.trim().is_empty()
    {
        return Err(ExportError::InvalidState("导出主题元数据无效".into()));
    }
    Ok(())
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

fn slot_path(slot: AppearanceSlot) -> String {
    slot.as_str().replace('.', "-")
}

fn hash_file(path: &Path) -> Result<(String, u64), ExportError> {
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
        hasher.update(&buffer[..read]);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}
