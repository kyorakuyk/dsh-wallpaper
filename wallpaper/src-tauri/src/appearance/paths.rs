use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearancePaths {
    pub root: PathBuf,
    pub catalog: PathBuf,
    pub settings: PathBuf,
    pub library: PathBuf,
    pub objects_sha256: PathBuf,
    pub themes: PathBuf,
    pub inbox: PathBuf,
    pub previews: PathBuf,
    pub staging: PathBuf,
    pub exports: PathBuf,
    pub diagnostics: PathBuf,
}

impl AppearancePaths {
    pub fn from_local_app_data() -> Option<Self> {
        dirs::data_local_dir().map(|directory| Self::new(directory.join("dsh-wallpaper")))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let library = root.join("library");
        Self {
            catalog: root.join("catalog.db"),
            settings: root.join("settings.json"),
            objects_sha256: library.join("objects").join("sha256"),
            themes: library.join("themes"),
            inbox: library.join("inbox"),
            previews: library.join("previews"),
            staging: library.join("staging"),
            exports: root.join("exports"),
            diagnostics: root.join("diagnostics"),
            library,
            root,
        }
    }

    pub fn create(&self) -> io::Result<()> {
        for directory in [
            &self.root,
            &self.objects_sha256,
            &self.themes,
            &self.inbox,
            &self.previews,
            &self.staging,
            &self.exports,
            &self.diagnostics,
        ] {
            fs::create_dir_all(directory)?;
        }
        Ok(())
    }

    pub fn object_path(&self, sha256: &str, extension: &str) -> Option<PathBuf> {
        if sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || extension.is_empty()
            || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return None;
        }
        let normalized_hash = sha256.to_ascii_lowercase();
        Some(
            self.objects_sha256
                .join(&normalized_hash[..2])
                .join(format!(
                    "{normalized_hash}.{}",
                    extension.to_ascii_lowercase()
                )),
        )
    }

    pub fn theme_manifest_path(&self, id: &str, version: &str) -> PathBuf {
        self.themes.join(id).join(version).join("theme.json")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
