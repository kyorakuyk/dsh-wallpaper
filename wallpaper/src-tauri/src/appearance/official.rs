//! Metadata for the immutable appearance baseline bundled with the app.
//!
//! The public images live in the application bundle and are already the
//! runtime fallback for the desktop scene.  Do not mirror them into
//! `%LOCALAPPDATA%`: the catalog entry below deliberately has no asset rows or
//! `theme_files` rows.  That keeps the official baseline visible in the same
//! theme picker as user packages without making it look like user-imported
//! loose material.

use super::types::{ThemeRecord, ThemeSource};

pub const OFFICIAL_BASE_THEME_ID: &str = "official.deepsea";
pub const OFFICIAL_BASE_THEME_VERSION: &str = "1.0.0";

/// The result of attempting the one-time runtime bootstrap.
///
/// `PreservedExistingCatalog` is intentionally a success: a catalog which
/// already contains user data or a selected theme must never be rewritten just
/// because a later app version learned about the official baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialThemeBootstrapOutcome {
    Initialized,
    AlreadyInitialized,
    PreservedExistingCatalog,
}

/// Returns the metadata-only official theme record.
///
/// `manifest_path` is a non-filesystem builtin resource identifier.  It must
/// not be resolved relative to the user appearance directory; actual scene
/// resources continue to be supplied by the packaged frontend fallback.
pub fn official_base_theme() -> ThemeRecord {
    ThemeRecord {
        id: OFFICIAL_BASE_THEME_ID.into(),
        version: OFFICIAL_BASE_THEME_VERSION.into(),
        name: "深夜工作室".into(),
        author: Some("DSH Wallpaper".into()),
        description: Some("随应用提供的只读默认外观。".into()),
        // This is a Vite public-resource path, not a path in the user library.
        // Consumers that cannot safely resolve a bundled preview should omit it
        // rather than exposing a raw local path.
        preview: Some("personas/deepsea-bg/deepsea-studio.png".into()),
        source: ThemeSource::Official,
        manifest_path: format!(
            "builtin://dsh-wallpaper/{OFFICIAL_BASE_THEME_ID}/{OFFICIAL_BASE_THEME_VERSION}/theme.json"
        ),
        readonly: true,
        installed_at: None,
        baseline_id: OFFICIAL_BASE_THEME_ID.into(),
        baseline_version: OFFICIAL_BASE_THEME_VERSION.into(),
    }
}
