use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AppearanceSlot {
    DesktopBackground,
    LockscreenImage,
    WakeSequence,
    PersonaDeepseekFlash,
    PersonaDeepseekPro,
    PersonaHarnessFlash,
    PersonaHarnessPro,
    ChatSkin,
    UiFont,
}

impl AppearanceSlot {
    pub const ALL: [Self; 9] = [
        Self::DesktopBackground,
        Self::LockscreenImage,
        Self::WakeSequence,
        Self::PersonaDeepseekFlash,
        Self::PersonaDeepseekPro,
        Self::PersonaHarnessFlash,
        Self::PersonaHarnessPro,
        Self::ChatSkin,
        Self::UiFont,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesktopBackground => "desktop.background",
            Self::LockscreenImage => "lockscreen.image",
            Self::WakeSequence => "wake.sequence",
            Self::PersonaDeepseekFlash => "persona.deepseek.flash",
            Self::PersonaDeepseekPro => "persona.deepseek.pro",
            Self::PersonaHarnessFlash => "persona.harness.flash",
            Self::PersonaHarnessPro => "persona.harness.pro",
            Self::ChatSkin => "chat.skin",
            Self::UiFont => "ui.font",
        }
    }
}

impl fmt::Display for AppearanceSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AppearanceSlot {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|slot| slot.as_str() == value)
            .ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetMediaType {
    Image,
    Font,
    Sequence,
    Skin,
}

impl AssetMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Font => "font",
            Self::Sequence => "sequence",
            Self::Skin => "skin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetStatus {
    Inbox,
    Classified,
    Corrupt,
}

impl AssetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Classified => "classified",
            Self::Corrupt => "corrupt",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetOrigin {
    Loose,
    ThemePrivate {
        theme_id: String,
        theme_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRecord {
    pub id: String,
    pub sha256: String,
    pub media_type: AssetMediaType,
    pub original_name: String,
    pub object_path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_alpha: Option<bool>,
    pub status: AssetStatus,
    pub slots: Vec<AppearanceSlot>,
    pub origin: AssetOrigin,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSource {
    Official,
    User,
}

impl ThemeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeRecord {
    pub id: String,
    pub version: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub preview: Option<String>,
    pub source: ThemeSource,
    pub manifest_path: String,
    pub readonly: bool,
    pub installed_at: Option<i64>,
    pub baseline_id: String,
    pub baseline_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeAssetLink {
    pub theme_id: String,
    pub theme_version: String,
    pub slot: AppearanceSlot,
    pub package_path: String,
    pub asset_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFileRecord {
    pub theme_id: String,
    pub theme_version: String,
    pub package_path: String,
    pub sha256: String,
    pub size: u64,
    pub asset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTheme {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertAssetOutcome {
    Inserted(AssetRecord),
    Existing(AssetRecord),
}
