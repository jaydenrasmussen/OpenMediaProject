use omp_core::{language::Language, quality::Quality, release::DownloadProtocol};
use omp_quality::{
    custom_format::{CustomFormat, CustomFormatDef},
    definition::{QualityDefinition, QualityDefinitions},
    delay::DelayProfile,
    profile::{ProfileFormatItem, ProfileItem, QualityProfile},
    release_profile::ReleaseProfile,
};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow, types::Json};

use crate::{DbError, Result};

fn language(id: i32) -> Result<Language> {
    Language::from_id(id).ok_or_else(|| DbError::Corrupt(format!("unknown language id {id}")))
}

fn protocol(value: i32) -> DownloadProtocol {
    match value {
        1 => DownloadProtocol::Usenet,
        2 => DownloadProtocol::Torrent,
        _ => DownloadProtocol::Unknown,
    }
}

fn protocol_value(p: DownloadProtocol) -> i32 {
    match p {
        DownloadProtocol::Unknown => 0,
        DownloadProtocol::Usenet => 1,
        DownloadProtocol::Torrent => 2,
    }
}

fn not_found_if_none(rows: u64) -> Result<()> {
    if rows == 0 {
        Err(DbError::NotFound)
    } else {
        Ok(())
    }
}

// ---- quality profiles -----------------------------------------------------------

pub struct QualityProfiles<'a>(pub(crate) &'a SqlitePool);

fn profile_from_row(r: &SqliteRow) -> Result<QualityProfile> {
    Ok(QualityProfile {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        cutoff: r.try_get::<i64, _>("cutoff")? as u32,
        upgrade_allowed: r.try_get("upgrade_allowed")?,
        min_format_score: r.try_get("min_format_score")?,
        cutoff_format_score: r.try_get("cutoff_format_score")?,
        min_upgrade_format_score: r.try_get("min_upgrade_format_score")?,
        language: language(r.try_get("language_id")?)?,
        items: r.try_get::<Json<Vec<ProfileItem>>, _>("items")?.0,
        format_items: r
            .try_get::<Json<Vec<ProfileFormatItem>>, _>("format_items")?
            .0,
    })
}

impl QualityProfiles<'_> {
    pub async fn list(&self) -> Result<Vec<QualityProfile>> {
        sqlx::query("SELECT * FROM quality_profiles ORDER BY name")
            .fetch_all(self.0)
            .await?
            .iter()
            .map(profile_from_row)
            .collect()
    }

    pub async fn get(&self, id: i32) -> Result<QualityProfile> {
        let row = sqlx::query("SELECT * FROM quality_profiles WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)?;
        profile_from_row(&row)
    }

    /// Inserts the profile; its `id` is ignored and the new id returned.
    pub async fn insert(&self, p: &QualityProfile) -> Result<i32> {
        let id = sqlx::query(
            "INSERT INTO quality_profiles
             (name, cutoff, upgrade_allowed, min_format_score, cutoff_format_score,
              min_upgrade_format_score, language_id, items, format_items)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&p.name)
        .bind(i64::from(p.cutoff))
        .bind(p.upgrade_allowed)
        .bind(p.min_format_score)
        .bind(p.cutoff_format_score)
        .bind(p.min_upgrade_format_score)
        .bind(p.language.id)
        .bind(Json(&p.items))
        .bind(Json(&p.format_items))
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id as i32)
    }

    pub async fn update(&self, p: &QualityProfile) -> Result<()> {
        let done = sqlx::query(
            "UPDATE quality_profiles SET name = ?, cutoff = ?, upgrade_allowed = ?,
             min_format_score = ?, cutoff_format_score = ?, min_upgrade_format_score = ?,
             language_id = ?, items = ?, format_items = ? WHERE id = ?",
        )
        .bind(&p.name)
        .bind(i64::from(p.cutoff))
        .bind(p.upgrade_allowed)
        .bind(p.min_format_score)
        .bind(p.cutoff_format_score)
        .bind(p.min_upgrade_format_score)
        .bind(p.language.id)
        .bind(Json(&p.items))
        .bind(Json(&p.format_items))
        .bind(p.id)
        .execute(self.0)
        .await?;
        not_found_if_none(done.rows_affected())
    }

    /// Fails with [`DbError::Constraint`] while a movie still uses the profile.
    pub async fn delete(&self, id: i32) -> Result<()> {
        let done = sqlx::query("DELETE FROM quality_profiles WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        not_found_if_none(done.rows_affected())
    }
}

// ---- quality definitions ---------------------------------------------------------

pub struct QualityDefinitionsRepo<'a>(pub(crate) &'a SqlitePool);

impl QualityDefinitionsRepo<'_> {
    /// Stored definitions, falling back to Radarr's defaults for any quality
    /// that has no row (a new quality added in an upgrade, say).
    pub async fn load(&self) -> Result<QualityDefinitions> {
        let rows = sqlx::query(
            "SELECT quality_id, weight, min_size, max_size, preferred_size FROM quality_definitions",
        )
        .fetch_all(self.0)
        .await?;
        let mut stored = Vec::new();
        for r in rows {
            let id: i64 = r.try_get("quality_id")?;
            let quality = Quality::from_id(id as u32)
                .ok_or_else(|| DbError::Corrupt(format!("unknown quality id {id}")))?;
            stored.push(QualityDefinition {
                quality,
                weight: r.try_get::<i64, _>("weight")? as u32,
                min_size: r.try_get("min_size")?,
                max_size: r.try_get("max_size")?,
                preferred_size: r.try_get("preferred_size")?,
            });
        }
        let mut all = QualityDefinitions::defaults().all().to_vec();
        for d in &mut all {
            if let Some(s) = stored.iter().find(|s| s.quality == d.quality) {
                *d = s.clone();
            }
        }
        Ok(QualityDefinitions::new(all))
    }

    pub async fn save(&self, d: &QualityDefinition) -> Result<()> {
        sqlx::query(
            "INSERT INTO quality_definitions (quality_id, weight, min_size, max_size, preferred_size)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(quality_id) DO UPDATE SET weight = excluded.weight,
               min_size = excluded.min_size, max_size = excluded.max_size,
               preferred_size = excluded.preferred_size",
        )
        .bind(i64::from(d.quality.id))
        .bind(i64::from(d.weight))
        .bind(d.min_size)
        .bind(d.max_size)
        .bind(d.preferred_size)
        .execute(self.0)
        .await?;
        Ok(())
    }
}

// ---- custom formats --------------------------------------------------------------------

pub struct CustomFormats<'a>(pub(crate) &'a SqlitePool);

fn format_from_row(r: &SqliteRow) -> Result<CustomFormatDef> {
    Ok(CustomFormatDef {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        include_when_renaming: r.try_get("include_when_renaming")?,
        specifications: r.try_get::<Json<Vec<_>>, _>("specifications")?.0,
    })
}

impl CustomFormats<'_> {
    pub async fn list(&self) -> Result<Vec<CustomFormatDef>> {
        sqlx::query("SELECT * FROM custom_formats ORDER BY name")
            .fetch_all(self.0)
            .await?
            .iter()
            .map(format_from_row)
            .collect()
    }

    /// Compiled formats, ready for matching. A stored format that no longer
    /// compiles is an error, not silently dropped: scores depend on it.
    pub async fn load_compiled(&self) -> Result<Vec<CustomFormat>> {
        self.list()
            .await?
            .iter()
            .map(|d| {
                d.build()
                    .map_err(|e| DbError::Corrupt(format!("custom format '{}': {e}", d.name)))
            })
            .collect()
    }

    /// Validates the conditions (regexes compile) before saving.
    pub async fn insert(&self, f: &CustomFormatDef) -> Result<i32> {
        f.build().map_err(DbError::Constraint)?;
        let id = sqlx::query(
            "INSERT INTO custom_formats (name, include_when_renaming, specifications) VALUES (?, ?, ?)",
        )
        .bind(&f.name)
        .bind(f.include_when_renaming)
        .bind(Json(&f.specifications))
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id as i32)
    }

    pub async fn update(&self, f: &CustomFormatDef) -> Result<()> {
        f.build().map_err(DbError::Constraint)?;
        let done = sqlx::query(
            "UPDATE custom_formats SET name = ?, include_when_renaming = ?, specifications = ? WHERE id = ?",
        )
        .bind(&f.name)
        .bind(f.include_when_renaming)
        .bind(Json(&f.specifications))
        .bind(f.id)
        .execute(self.0)
        .await?;
        not_found_if_none(done.rows_affected())
    }

    pub async fn delete(&self, id: i32) -> Result<()> {
        let done = sqlx::query("DELETE FROM custom_formats WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        not_found_if_none(done.rows_affected())
    }
}

// ---- delay profiles ----------------------------------------------------------------------

pub struct DelayProfiles<'a>(pub(crate) &'a SqlitePool);

fn delay_from_row(r: &SqliteRow) -> Result<DelayProfile> {
    Ok(DelayProfile {
        id: r.try_get("id")?,
        enable_usenet: r.try_get("enable_usenet")?,
        enable_torrent: r.try_get("enable_torrent")?,
        preferred_protocol: protocol(r.try_get("preferred_protocol")?),
        usenet_delay: r.try_get("usenet_delay")?,
        torrent_delay: r.try_get("torrent_delay")?,
        order: r.try_get("sort_order")?,
        bypass_if_highest_quality: r.try_get("bypass_if_highest_quality")?,
        bypass_if_above_custom_format_score: r.try_get("bypass_if_above_custom_format_score")?,
        minimum_custom_format_score: r.try_get("minimum_custom_format_score")?,
        tags: r.try_get::<Json<Vec<i32>>, _>("tags")?.0,
    })
}

impl DelayProfiles<'_> {
    /// All profiles, lowest `order` first.
    pub async fn list(&self) -> Result<Vec<DelayProfile>> {
        sqlx::query("SELECT * FROM delay_profiles ORDER BY sort_order")
            .fetch_all(self.0)
            .await?
            .iter()
            .map(delay_from_row)
            .collect()
    }

    /// Creates the catch-all profile if there are no profiles at all.
    pub async fn seed_default(&self) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delay_profiles")
            .fetch_one(self.0)
            .await?;
        if count == 0 {
            self.insert(&DelayProfile::default_profile()).await?;
        }
        Ok(())
    }

    pub async fn insert(&self, p: &DelayProfile) -> Result<i32> {
        let id = sqlx::query(
            "INSERT INTO delay_profiles
             (enable_usenet, enable_torrent, preferred_protocol, usenet_delay, torrent_delay,
              sort_order, bypass_if_highest_quality, bypass_if_above_custom_format_score,
              minimum_custom_format_score, tags)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(p.enable_usenet)
        .bind(p.enable_torrent)
        .bind(protocol_value(p.preferred_protocol))
        .bind(p.usenet_delay)
        .bind(p.torrent_delay)
        .bind(p.order)
        .bind(p.bypass_if_highest_quality)
        .bind(p.bypass_if_above_custom_format_score)
        .bind(p.minimum_custom_format_score)
        .bind(Json(&p.tags))
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id as i32)
    }

    pub async fn update(&self, p: &DelayProfile) -> Result<()> {
        let done = sqlx::query(
            "UPDATE delay_profiles SET enable_usenet = ?, enable_torrent = ?, preferred_protocol = ?,
             usenet_delay = ?, torrent_delay = ?, sort_order = ?, bypass_if_highest_quality = ?,
             bypass_if_above_custom_format_score = ?, minimum_custom_format_score = ?, tags = ?
             WHERE id = ?",
        )
        .bind(p.enable_usenet)
        .bind(p.enable_torrent)
        .bind(protocol_value(p.preferred_protocol))
        .bind(p.usenet_delay)
        .bind(p.torrent_delay)
        .bind(p.order)
        .bind(p.bypass_if_highest_quality)
        .bind(p.bypass_if_above_custom_format_score)
        .bind(p.minimum_custom_format_score)
        .bind(Json(&p.tags))
        .bind(p.id)
        .execute(self.0)
        .await?;
        not_found_if_none(done.rows_affected())
    }

    /// The catch-all profile (no tags, last in order) cannot be deleted:
    /// every movie needs one that matches.
    pub async fn delete(&self, id: i32) -> Result<()> {
        let row = sqlx::query("SELECT * FROM delay_profiles WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)?;
        let profile = delay_from_row(&row)?;
        if profile.tags.is_empty() && profile.order == i32::MAX {
            return Err(DbError::Constraint(
                "the default delay profile cannot be deleted".into(),
            ));
        }
        sqlx::query("DELETE FROM delay_profiles WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        Ok(())
    }
}

// ---- release profiles --------------------------------------------------------------------

pub struct ReleaseProfiles<'a>(pub(crate) &'a SqlitePool);

fn release_profile_from_row(r: &SqliteRow) -> Result<ReleaseProfile> {
    Ok(ReleaseProfile {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        enabled: r.try_get("enabled")?,
        required: r.try_get::<Json<Vec<String>>, _>("required")?.0,
        ignored: r.try_get::<Json<Vec<String>>, _>("ignored")?.0,
        indexer_id: r.try_get("indexer_id")?,
        tags: r.try_get::<Json<Vec<i32>>, _>("tags")?.0,
        air_date_restriction: r.try_get("air_date_restriction")?,
        air_date_grace_period_hours: r.try_get("air_date_grace_period_hours")?,
    })
}

impl ReleaseProfiles<'_> {
    pub async fn list(&self) -> Result<Vec<ReleaseProfile>> {
        sqlx::query("SELECT * FROM release_profiles ORDER BY id")
            .fetch_all(self.0)
            .await?
            .iter()
            .map(release_profile_from_row)
            .collect()
    }

    pub async fn insert(&self, p: &ReleaseProfile) -> Result<i32> {
        let id = sqlx::query(
            "INSERT INTO release_profiles (name, enabled, required, ignored, indexer_id, tags,
             air_date_restriction, air_date_grace_period_hours)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&p.name)
        .bind(p.enabled)
        .bind(Json(&p.required))
        .bind(Json(&p.ignored))
        .bind(p.indexer_id)
        .bind(Json(&p.tags))
        .bind(p.air_date_restriction)
        .bind(p.air_date_grace_period_hours)
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id as i32)
    }

    pub async fn update(&self, p: &ReleaseProfile) -> Result<()> {
        let done = sqlx::query(
            "UPDATE release_profiles SET name = ?, enabled = ?, required = ?, ignored = ?,
             indexer_id = ?, tags = ?, air_date_restriction = ?, air_date_grace_period_hours = ?
             WHERE id = ?",
        )
        .bind(&p.name)
        .bind(p.enabled)
        .bind(Json(&p.required))
        .bind(Json(&p.ignored))
        .bind(p.indexer_id)
        .bind(Json(&p.tags))
        .bind(p.air_date_restriction)
        .bind(p.air_date_grace_period_hours)
        .bind(p.id)
        .execute(self.0)
        .await?;
        not_found_if_none(done.rows_affected())
    }

    pub async fn delete(&self, id: i32) -> Result<()> {
        let done = sqlx::query("DELETE FROM release_profiles WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        not_found_if_none(done.rows_affected())
    }
}
