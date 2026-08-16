use std::error::Error;
use std::fmt;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::types::{
    ActiveTheme, AppearanceSlot, AssetMediaType, AssetOrigin, AssetRecord, AssetStatus,
    InsertAssetOutcome, ThemeAssetLink, ThemeFileRecord, ThemeRecord,
};

const SCHEMA_VERSION: i32 = 3;

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    InvalidRecord(String),
    NotFound(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "appearance database error: {error}"),
            Self::InvalidRecord(message) => formatter.write_str(message),
            Self::NotFound(message) => formatter.write_str(message),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidRecord(_) | Self::NotFound(_) => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

pub struct AppearanceRepository {
    connection: Connection,
}

impl AppearanceRepository {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut repository = Self { connection };
        repository.migrate()?;
        Ok(repository)
    }

    fn migrate(&mut self) -> Result<(), StoreError> {
        let current: i32 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current > SCHEMA_VERSION {
            return Err(StoreError::InvalidRecord(format!(
                "catalog schema {current} is newer than supported version {SCHEMA_VERSION}"
            )));
        }
        if current == SCHEMA_VERSION {
            return Ok(());
        }

        let transaction = self.connection.transaction()?;
        if current == 0 {
            transaction.execute_batch(
            "
            CREATE TABLE themes (
                id TEXT NOT NULL,
                version TEXT NOT NULL,
                name TEXT NOT NULL,
                author TEXT,
                description TEXT,
                preview TEXT,
                source TEXT NOT NULL CHECK(source IN ('official', 'user')),
                manifest_path TEXT NOT NULL,
                readonly INTEGER NOT NULL CHECK(readonly IN (0, 1)),
                installed_at INTEGER,
                baseline_id TEXT NOT NULL,
                baseline_version TEXT NOT NULL,
                PRIMARY KEY(id, version)
            );

            CREATE TABLE assets (
                id TEXT PRIMARY KEY,
                sha256 TEXT NOT NULL UNIQUE CHECK(length(sha256) = 64),
                media_type TEXT NOT NULL CHECK(media_type IN ('image', 'font', 'sequence', 'skin')),
                original_name TEXT NOT NULL,
                object_path TEXT NOT NULL,
                width INTEGER,
                height INTEGER,
                has_alpha INTEGER CHECK(has_alpha IS NULL OR has_alpha IN (0, 1)),
                status TEXT NOT NULL CHECK(status IN ('inbox', 'classified', 'corrupt')),
                origin_kind TEXT NOT NULL CHECK(origin_kind IN ('loose', 'theme-private')),
                origin_theme_id TEXT,
                origin_theme_version TEXT,
                created_at INTEGER NOT NULL,
                CHECK(
                    (origin_kind = 'loose' AND origin_theme_id IS NULL AND origin_theme_version IS NULL)
                    OR
                    (origin_kind = 'theme-private' AND origin_theme_id IS NOT NULL AND origin_theme_version IS NOT NULL)
                ),
                FOREIGN KEY(origin_theme_id, origin_theme_version)
                    REFERENCES themes(id, version) ON DELETE CASCADE
            );

            CREATE TABLE asset_slots (
                asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                slot TEXT NOT NULL CHECK(slot IN (
                    'desktop.background', 'lockscreen.image', 'wake.sequence',
                    'persona.deepseek.flash', 'persona.deepseek.pro',
                    'persona.harness.flash', 'persona.harness.pro',
                    'chat.skin', 'ui.font'
                )),
                PRIMARY KEY(asset_id, slot)
            );

            CREATE TABLE theme_files (
                theme_id TEXT NOT NULL,
                theme_version TEXT NOT NULL,
                package_path TEXT NOT NULL,
                sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
                size INTEGER NOT NULL CHECK(size >= 0),
                asset_id TEXT REFERENCES assets(id) ON DELETE RESTRICT,
                PRIMARY KEY(theme_id, theme_version, package_path),
                FOREIGN KEY(theme_id, theme_version) REFERENCES themes(id, version) ON DELETE CASCADE
            );

            CREATE TABLE theme_assets (
                theme_id TEXT NOT NULL,
                theme_version TEXT NOT NULL,
                slot TEXT NOT NULL,
                package_path TEXT NOT NULL,
                asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE RESTRICT,
                PRIMARY KEY(theme_id, theme_version, slot, package_path),
                FOREIGN KEY(theme_id, theme_version) REFERENCES themes(id, version) ON DELETE CASCADE,
                FOREIGN KEY(theme_id, theme_version, package_path)
                    REFERENCES theme_files(theme_id, theme_version, package_path) ON DELETE CASCADE
            );

            CREATE TABLE appearance_state (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                active_theme_id TEXT,
                active_theme_version TEXT,
                FOREIGN KEY(active_theme_id, active_theme_version) REFERENCES themes(id, version) ON DELETE RESTRICT,
                CHECK(
                    (active_theme_id IS NULL AND active_theme_version IS NULL)
                    OR
                    (active_theme_id IS NOT NULL AND active_theme_version IS NOT NULL)
                )
            );

            CREATE TABLE overrides (
                slot TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE RESTRICT
            );

            CREATE INDEX assets_library_lookup
                ON assets(origin_kind, status);
            CREATE INDEX asset_slots_slot_lookup
                ON asset_slots(slot, asset_id);

            INSERT INTO appearance_state(singleton, active_theme_id, active_theme_version)
            VALUES (1, NULL, NULL);
            PRAGMA user_version = 3;
            ",
            )?;
        } else if current == 1 {
            transaction.execute_batch(
                "
                ALTER TABLE themes ADD COLUMN name TEXT NOT NULL DEFAULT '';
                ALTER TABLE themes ADD COLUMN author TEXT;
                ALTER TABLE themes ADD COLUMN description TEXT;
                ALTER TABLE themes ADD COLUMN preview TEXT;
                UPDATE themes SET name = id WHERE name = '';
                ALTER TABLE theme_assets RENAME TO theme_assets_v2;
                CREATE TABLE theme_assets (
                    theme_id TEXT NOT NULL,
                    theme_version TEXT NOT NULL,
                    slot TEXT NOT NULL,
                    package_path TEXT NOT NULL,
                    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE RESTRICT,
                    PRIMARY KEY(theme_id, theme_version, slot, package_path),
                    FOREIGN KEY(theme_id, theme_version) REFERENCES themes(id, version) ON DELETE CASCADE,
                    FOREIGN KEY(theme_id, theme_version, package_path)
                        REFERENCES theme_files(theme_id, theme_version, package_path) ON DELETE CASCADE
                );
                INSERT INTO theme_assets SELECT * FROM theme_assets_v2;
                DROP TABLE theme_assets_v2;
                PRAGMA user_version = 3;
                ",
            )?;
        } else if current == 2 {
            transaction.execute_batch(
                "
                ALTER TABLE theme_assets RENAME TO theme_assets_v2;
                CREATE TABLE theme_assets (
                    theme_id TEXT NOT NULL,
                    theme_version TEXT NOT NULL,
                    slot TEXT NOT NULL,
                    package_path TEXT NOT NULL,
                    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE RESTRICT,
                    PRIMARY KEY(theme_id, theme_version, slot, package_path),
                    FOREIGN KEY(theme_id, theme_version) REFERENCES themes(id, version) ON DELETE CASCADE,
                    FOREIGN KEY(theme_id, theme_version, package_path)
                        REFERENCES theme_files(theme_id, theme_version, package_path) ON DELETE CASCADE
                );
                INSERT INTO theme_assets SELECT * FROM theme_assets_v2;
                DROP TABLE theme_assets_v2;
                PRAGMA user_version = 3;
                ",
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i32, StoreError> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn insert_theme(&mut self, theme: &ThemeRecord) -> Result<(), StoreError> {
        validate_theme(theme)?;
        self.connection.execute(
            "INSERT INTO themes(
                id, version, name, author, description, preview, source, manifest_path,
                readonly, installed_at, baseline_id, baseline_version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                theme.id,
                theme.version,
                theme.name,
                theme.author,
                theme.description,
                theme.preview,
                theme.source.as_str(),
                theme.manifest_path,
                theme.readonly,
                theme.installed_at,
                theme.baseline_id,
                theme.baseline_version,
            ],
        )?;
        Ok(())
    }

    pub fn install_verified_theme(
        &mut self,
        theme: &ThemeRecord,
        assets: &[AssetRecord],
        files: &[ThemeFileRecord],
        links: &[ThemeAssetLink],
    ) -> Result<(), StoreError> {
        validate_theme(theme)?;
        for asset in assets {
            validate_asset(asset)?;
            match &asset.origin {
                AssetOrigin::ThemePrivate {
                    theme_id,
                    theme_version,
                } if theme_id == &theme.id && theme_version == &theme.version => {}
                AssetOrigin::ThemePrivate { .. } => {
                    return Err(StoreError::InvalidRecord(
                        "theme-private asset owner does not match installed theme".into(),
                    ));
                }
                AssetOrigin::Loose => {}
            }
        }
        if files
            .iter()
            .any(|file| file.theme_id != theme.id || file.theme_version != theme.version)
            || links
                .iter()
                .any(|link| link.theme_id != theme.id || link.theme_version != theme.version)
        {
            return Err(StoreError::InvalidRecord(
                "theme file and component owners must match the installed theme".into(),
            ));
        }

        let transaction = self.connection.transaction()?;
        insert_theme_tx(&transaction, theme)?;
        for asset in assets {
            insert_asset_tx(&transaction, asset)?;
        }
        for file in files {
            transaction.execute(
                "INSERT INTO theme_files(theme_id, theme_version, package_path, sha256, size, asset_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    file.theme_id,
                    file.theme_version,
                    file.package_path,
                    file.sha256,
                    i64::try_from(file.size).map_err(|_| StoreError::InvalidRecord("theme file is too large".into()))?,
                    file.asset_id,
                ],
            )?;
        }
        for link in links {
            transaction.execute(
                "INSERT INTO theme_assets(theme_id, theme_version, slot, package_path, asset_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    link.theme_id,
                    link.theme_version,
                    link.slot.as_str(),
                    link.package_path,
                    link.asset_id,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn insert_asset(&mut self, asset: &AssetRecord) -> Result<InsertAssetOutcome, StoreError> {
        validate_asset(asset)?;
        if let Some(existing) = self.asset_by_sha256(&asset.sha256)? {
            return Ok(InsertAssetOutcome::Existing(existing));
        }

        let transaction = self.connection.transaction()?;
        insert_asset_tx(&transaction, asset)?;
        transaction.commit()?;
        Ok(InsertAssetOutcome::Inserted(asset.clone()))
    }

    pub fn theme_exists(&self, id: &str, version: &str) -> Result<bool, StoreError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM themes WHERE id = ?1 AND version = ?2)",
            params![id, version],
            |row| row.get(0),
        )?)
    }

    pub fn asset_by_sha256(&self, sha256: &str) -> Result<Option<AssetRecord>, StoreError> {
        let id = self
            .connection
            .query_row("SELECT id FROM assets WHERE sha256 = ?1", [sha256], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        id.map(|id| self.asset_by_id(&id)).transpose()
    }

    pub fn asset_by_id(&self, id: &str) -> Result<AssetRecord, StoreError> {
        self.load_asset_by_id(id)
    }

    pub fn classify_asset(
        &mut self,
        asset_id: &str,
        slots: &[AppearanceSlot],
    ) -> Result<AssetRecord, StoreError> {
        if slots.is_empty() {
            return Err(StoreError::InvalidRecord(
                "classified assets require at least one slot".into(),
            ));
        }
        let mut unique_slots = slots.to_vec();
        unique_slots.sort_unstable();
        unique_slots.dedup();

        let current = self.load_asset_by_id(asset_id)?;
        if current.origin != AssetOrigin::Loose || current.status == AssetStatus::Corrupt {
            return Err(StoreError::InvalidRecord(
                "only non-corrupt loose assets can be classified".into(),
            ));
        }
        if unique_slots
            .iter()
            .any(|slot| !media_supports_slot(current.media_type, *slot))
        {
            return Err(StoreError::InvalidRecord(
                "asset media type is incompatible with a requested slot".into(),
            ));
        }

        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM asset_slots WHERE asset_id = ?1", [asset_id])?;
        for slot in &unique_slots {
            transaction.execute(
                "INSERT INTO asset_slots(asset_id, slot) VALUES (?1, ?2)",
                params![asset_id, slot.as_str()],
            )?;
        }
        let changed = transaction.execute(
            "UPDATE assets SET status = 'classified' WHERE id = ?1",
            [asset_id],
        )?;
        if changed != 1 {
            return Err(StoreError::NotFound("asset not found".into()));
        }
        transaction.commit()?;
        self.load_asset_by_id(asset_id)
    }

    pub fn theme_by_id_version(&self, id: &str, version: &str) -> Result<ThemeRecord, StoreError> {
        self.connection
            .query_row(
                "SELECT id, version, name, author, description, preview, source, manifest_path,
                        readonly, installed_at, baseline_id, baseline_version
                 FROM themes WHERE id = ?1 AND version = ?2",
                params![id, version],
                theme_from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("theme not found".into()))
    }

    pub fn theme_component_assets(
        &self,
        id: &str,
        version: &str,
        slot: AppearanceSlot,
    ) -> Result<Vec<(String, AssetRecord)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT package_path, asset_id
             FROM theme_assets
             WHERE theme_id = ?1 AND theme_version = ?2 AND slot = ?3
             ORDER BY package_path",
        )?;
        let rows = statement
            .query_map(params![id, version, slot.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(path, asset_id)| Ok((path, self.load_asset_by_id(&asset_id)?)))
            .collect()
    }

    pub fn resolve_active_asset(
        &self,
        slot: AppearanceSlot,
    ) -> Result<Option<AssetRecord>, StoreError> {
        if let Some(asset_id) = self.override_asset_id(slot)? {
            return self.asset_by_id(&asset_id).map(Some);
        }
        let Some(active) = self.active_theme()? else {
            return Ok(None);
        };
        self.resolve_theme_asset(&active.id, &active.version, slot, &mut std::collections::BTreeSet::new())
    }

    fn resolve_theme_asset(
        &self,
        id: &str,
        version: &str,
        slot: AppearanceSlot,
        visited: &mut std::collections::BTreeSet<(String, String)>,
    ) -> Result<Option<AssetRecord>, StoreError> {
        if !visited.insert((id.to_owned(), version.to_owned())) {
            return Err(StoreError::InvalidRecord("theme baseline cycle".into()));
        }
        if let Some((_, asset)) = self
            .theme_component_assets(id, version, slot)?
            .into_iter()
            .next()
        {
            return Ok(Some(asset));
        }
        let theme = self.theme_by_id_version(id, version)?;
        if theme.baseline_id == id && theme.baseline_version == version {
            return Ok(None);
        }
        if !self.theme_exists(&theme.baseline_id, &theme.baseline_version)? {
            return Ok(None);
        }
        self.resolve_theme_asset(
            &theme.baseline_id,
            &theme.baseline_version,
            slot,
            visited,
        )
    }

    pub fn list_component_assets(
        &self,
        slot: AppearanceSlot,
    ) -> Result<Vec<AssetRecord>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT a.id
             FROM assets a
             JOIN asset_slots s ON s.asset_id = a.id
             WHERE a.origin_kind = 'loose' AND a.status = 'classified' AND s.slot = ?1
             ORDER BY a.created_at DESC, a.id ASC",
        )?;
        let ids = statement
            .query_map([slot.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.load_asset_by_id(&id))
            .collect()
    }

    pub fn list_library_assets(
        &self,
        slot: Option<AppearanceSlot>,
    ) -> Result<Vec<AssetRecord>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT a.id
             FROM assets a
             LEFT JOIN asset_slots s ON s.asset_id = a.id
             WHERE a.origin_kind = 'loose' AND (?1 IS NULL OR s.slot = ?1)
             ORDER BY a.created_at DESC, a.id ASC",
        )?;
        let slot = slot.map(|slot| slot.as_str());
        let ids = statement
            .query_map([slot], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.load_asset_by_id(&id))
            .collect()
    }

    pub fn list_themes(&self) -> Result<Vec<ThemeRecord>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id, version, name, author, description, preview, source, manifest_path,
                    readonly, installed_at, baseline_id, baseline_version
             FROM themes
             ORDER BY CASE source WHEN 'official' THEN 0 ELSE 1 END, id ASC, version DESC",
        )?;
        let themes = statement
            .query_map([], theme_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(themes)
    }

    pub fn set_override(&mut self, slot: AppearanceSlot, asset_id: &str) -> Result<(), StoreError> {
        let asset = self.load_asset_by_id(asset_id)?;
        if asset.origin != AssetOrigin::Loose
            || asset.status != AssetStatus::Classified
            || !asset.slots.contains(&slot)
        {
            return Err(StoreError::InvalidRecord(
                "override asset must be a classified loose asset assigned to the requested slot"
                    .into(),
            ));
        }
        self.connection.execute(
            "INSERT INTO overrides(slot, asset_id) VALUES (?1, ?2)
             ON CONFLICT(slot) DO UPDATE SET asset_id = excluded.asset_id",
            params![slot.as_str(), asset_id],
        )?;
        Ok(())
    }

    pub fn override_asset_id(&self, slot: AppearanceSlot) -> Result<Option<String>, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT asset_id FROM overrides WHERE slot = ?1",
                [slot.as_str()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn list_overrides(&self) -> Result<Vec<(AppearanceSlot, String)>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT slot, asset_id FROM overrides ORDER BY slot")?;
        let overrides = statement
            .query_map([], |row| {
                let slot: String = row.get(0)?;
                let parsed = slot.parse().map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok((parsed, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(overrides)
    }

    pub fn clear_override(&mut self, slot: Option<AppearanceSlot>) -> Result<(), StoreError> {
        match slot {
            Some(slot) => {
                self.connection
                    .execute("DELETE FROM overrides WHERE slot = ?1", [slot.as_str()])?;
            }
            None => {
                self.connection.execute("DELETE FROM overrides", [])?;
            }
        }
        Ok(())
    }

    pub fn activate_theme(&mut self, id: &str, version: &str) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM themes WHERE id = ?1 AND version = ?2)",
            params![id, version],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound("theme not found".into()));
        }
        transaction.execute("DELETE FROM overrides", [])?;
        let changed = transaction.execute(
            "UPDATE appearance_state
             SET active_theme_id = ?1, active_theme_version = ?2
             WHERE singleton = 1",
            params![id, version],
        )?;
        if changed != 1 {
            return Err(StoreError::NotFound("appearance state is missing".into()));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn active_theme(&self) -> Result<Option<ActiveTheme>, StoreError> {
        Ok(self.connection.query_row(
            "SELECT active_theme_id, active_theme_version FROM appearance_state WHERE singleton = 1",
            [],
            |row| {
                let id: Option<String> = row.get(0)?;
                let version: Option<String> = row.get(1)?;
                Ok(id.zip(version).map(|(id, version)| ActiveTheme { id, version }))
            },
        )?)
    }

    fn load_asset_by_id(&self, id: &str) -> Result<AssetRecord, StoreError> {
        let base = self
            .connection
            .query_row(
                "SELECT sha256, media_type, original_name, object_path, width, height, has_alpha,
                        status, origin_kind, origin_theme_id, origin_theme_version, created_at
                 FROM assets WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<u32>>(4)?,
                        row.get::<_, Option<u32>>(5)?,
                        row.get::<_, Option<bool>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, i64>(11)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("asset not found: {id}")))?;

        let mut statement = self
            .connection
            .prepare("SELECT slot FROM asset_slots WHERE asset_id = ?1 ORDER BY slot")?;
        let slots = statement
            .query_map([id], |row| row.get::<_, String>(0))?
            .map(|value| value?.parse().map_err(|_| rusqlite::Error::InvalidQuery))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(AssetRecord {
            id: id.to_owned(),
            sha256: base.0,
            media_type: parse_media_type(&base.1)?,
            original_name: base.2,
            object_path: base.3,
            width: base.4,
            height: base.5,
            has_alpha: base.6,
            status: parse_status(&base.7)?,
            slots,
            origin: match (base.8.as_str(), base.9, base.10) {
                ("loose", None, None) => AssetOrigin::Loose,
                ("theme-private", Some(theme_id), Some(theme_version)) => {
                    AssetOrigin::ThemePrivate {
                        theme_id,
                        theme_version,
                    }
                }
                _ => {
                    return Err(StoreError::InvalidRecord(
                        "stored asset origin is invalid".into(),
                    ))
                }
            },
            created_at: base.11,
        })
    }
}

fn media_supports_slot(media_type: AssetMediaType, slot: AppearanceSlot) -> bool {
    match media_type {
        AssetMediaType::Image => matches!(
            slot,
            AppearanceSlot::DesktopBackground
                | AppearanceSlot::LockscreenImage
                | AppearanceSlot::PersonaDeepseekFlash
                | AppearanceSlot::PersonaDeepseekPro
                | AppearanceSlot::PersonaHarnessFlash
                | AppearanceSlot::PersonaHarnessPro
        ),
        AssetMediaType::Font => slot == AppearanceSlot::UiFont,
        AssetMediaType::Sequence => slot == AppearanceSlot::WakeSequence,
        AssetMediaType::Skin => slot == AppearanceSlot::ChatSkin,
    }
}

fn theme_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThemeRecord> {
    let source: String = row.get(6)?;
    Ok(ThemeRecord {
        id: row.get(0)?,
        version: row.get(1)?,
        name: row.get(2)?,
        author: row.get(3)?,
        description: row.get(4)?,
        preview: row.get(5)?,
        source: match source.as_str() {
            "official" => super::types::ThemeSource::Official,
            "user" => super::types::ThemeSource::User,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        manifest_path: row.get(7)?,
        readonly: row.get(8)?,
        installed_at: row.get(9)?,
        baseline_id: row.get(10)?,
        baseline_version: row.get(11)?,
    })
}

fn validate_theme(theme: &ThemeRecord) -> Result<(), StoreError> {
    if theme.id.is_empty()
        || theme.version.is_empty()
        || theme.name.trim().is_empty()
        || theme.manifest_path.is_empty()
        || theme.baseline_id.is_empty()
        || theme.baseline_version.is_empty()
    {
        return Err(StoreError::InvalidRecord(
            "theme fields cannot be empty".into(),
        ));
    }
    if theme.source.as_str() == "official" && !theme.readonly {
        return Err(StoreError::InvalidRecord(
            "official themes must be readonly".into(),
        ));
    }
    Ok(())
}

fn validate_asset(asset: &AssetRecord) -> Result<(), StoreError> {
    if asset.id.is_empty() || asset.original_name.is_empty() || asset.object_path.is_empty() {
        return Err(StoreError::InvalidRecord(
            "asset fields cannot be empty".into(),
        ));
    }
    if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StoreError::InvalidRecord("asset sha256 is invalid".into()));
    }
    Ok(())
}

fn insert_theme_tx(transaction: &Transaction<'_>, theme: &ThemeRecord) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO themes(
            id, version, name, author, description, preview, source, manifest_path,
            readonly, installed_at, baseline_id, baseline_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            theme.id,
            theme.version,
            theme.name,
            theme.author,
            theme.description,
            theme.preview,
            theme.source.as_str(),
            theme.manifest_path,
            theme.readonly,
            theme.installed_at,
            theme.baseline_id,
            theme.baseline_version,
        ],
    )?;
    Ok(())
}

fn insert_asset_tx(transaction: &Transaction<'_>, asset: &AssetRecord) -> Result<(), StoreError> {
    let (origin_kind, origin_theme_id, origin_theme_version) = match &asset.origin {
        AssetOrigin::Loose => ("loose", None, None),
        AssetOrigin::ThemePrivate {
            theme_id,
            theme_version,
        } => (
            "theme-private",
            Some(theme_id.as_str()),
            Some(theme_version.as_str()),
        ),
    };
    transaction.execute(
        "INSERT INTO assets(
            id, sha256, media_type, original_name, object_path, width, height, has_alpha,
            status, origin_kind, origin_theme_id, origin_theme_version, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            asset.id,
            asset.sha256.to_ascii_lowercase(),
            asset.media_type.as_str(),
            asset.original_name,
            asset.object_path,
            asset.width,
            asset.height,
            asset.has_alpha,
            asset.status.as_str(),
            origin_kind,
            origin_theme_id,
            origin_theme_version,
            asset.created_at,
        ],
    )?;
    for slot in &asset.slots {
        transaction.execute(
            "INSERT INTO asset_slots(asset_id, slot) VALUES (?1, ?2)",
            params![asset.id, slot.as_str()],
        )?;
    }
    Ok(())
}

fn parse_media_type(value: &str) -> Result<AssetMediaType, StoreError> {
    match value {
        "image" => Ok(AssetMediaType::Image),
        "font" => Ok(AssetMediaType::Font),
        "sequence" => Ok(AssetMediaType::Sequence),
        "skin" => Ok(AssetMediaType::Skin),
        _ => Err(StoreError::InvalidRecord(
            "stored media type is invalid".into(),
        )),
    }
}

fn parse_status(value: &str) -> Result<AssetStatus, StoreError> {
    match value {
        "inbox" => Ok(AssetStatus::Inbox),
        "classified" => Ok(AssetStatus::Classified),
        "corrupt" => Ok(AssetStatus::Corrupt),
        _ => Err(StoreError::InvalidRecord(
            "stored asset status is invalid".into(),
        )),
    }
}
