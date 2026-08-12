//! SQLite-backed application data. Passwords remain in the native keyring.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tauri::{AppHandle, Manager};
use tauri_plugin_sql::{DbInstances, DbPool, Migration, MigrationKind};

pub const DATABASE_URL: &str = "sqlite:gp-client.db";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub portal: String,
    pub username: String,
    #[serde(default)]
    pub remember: bool,
    #[serde(default)]
    pub cafile: Option<String>,
    #[serde(default)]
    pub client_cert: Option<String>,
    #[serde(default)]
    pub client_key: Option<String>,
    #[serde(default)]
    pub trusted_fingerprint: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub client_os: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub hip: bool,
    #[serde(default)]
    pub requires_otp: bool,
}

pub fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "create application data",
            sql: r#"
            CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY NOT NULL,
                position INTEGER NOT NULL UNIQUE,
                name TEXT NOT NULL,
                portal TEXT NOT NULL,
                username TEXT NOT NULL,
                remember INTEGER NOT NULL DEFAULT 0 CHECK (remember IN (0, 1)),
                cafile TEXT,
                client_cert TEXT,
                client_key TEXT,
                trusted_fingerprint TEXT,
                gateway TEXT,
                client_os TEXT,
                script TEXT,
                hip INTEGER NOT NULL DEFAULT 0 CHECK (hip IN (0, 1)),
                requires_otp INTEGER NOT NULL DEFAULT 0 CHECK (requires_otp IN (0, 1))
            );

            CREATE TABLE IF NOT EXISTS network_totals (
                profile_id TEXT PRIMARY KEY NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                rx_bytes TEXT NOT NULL DEFAULT '0',
                tx_bytes TEXT NOT NULL DEFAULT '0',
                rx_packets TEXT NOT NULL DEFAULT '0',
                tx_packets TEXT NOT NULL DEFAULT '0'
            );

            CREATE TABLE IF NOT EXISTS preferences (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
        "#,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "remove network totals",
            sql: "DROP TABLE IF EXISTS network_totals;",
            kind: MigrationKind::Up,
        },
    ]
}

pub fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("p{nanos:x}")
}

pub fn default_name(portal: &str) -> String {
    let trimmed = portal
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let host = trimmed.split('/').next().unwrap_or(trimmed);
    if host.is_empty() {
        "New connection".into()
    } else {
        host.to_string()
    }
}

async fn pool(app: &AppHandle) -> Result<SqlitePool, String> {
    let instances = app.state::<DbInstances>();
    let instances = instances.0.read().await;
    match instances.get(DATABASE_URL) {
        Some(DbPool::Sqlite(pool)) => Ok(pool.clone()),
        _ => Err("Application database is not available".into()),
    }
}

fn profile_from_row(row: &sqlx::sqlite::SqliteRow) -> Profile {
    Profile {
        id: row.get("id"),
        name: row.get("name"),
        portal: row.get("portal"),
        username: row.get("username"),
        remember: row.get::<i64, _>("remember") != 0,
        cafile: row.get("cafile"),
        client_cert: row.get("client_cert"),
        client_key: row.get("client_key"),
        trusted_fingerprint: row.get("trusted_fingerprint"),
        gateway: row.get("gateway"),
        client_os: row.get("client_os"),
        script: row.get("script"),
        hip: row.get::<i64, _>("hip") != 0,
        requires_otp: row.get::<i64, _>("requires_otp") != 0,
    }
}

const PROFILE_COLUMNS: &str = "id, name, portal, username, remember, cafile, client_cert, client_key, trusted_fingerprint, gateway, client_os, script, hip, requires_otp";

pub async fn load(app: &AppHandle) -> Result<Vec<Profile>, String> {
    let rows = sqlx::query(&format!(
        "SELECT {PROFILE_COLUMNS} FROM profiles ORDER BY position"
    ))
    .fetch_all(&pool(app).await?)
    .await
    .map_err(|error| error.to_string())?;
    Ok(rows.iter().map(profile_from_row).collect())
}

pub async fn find(app: &AppHandle, id: &str) -> Result<Option<Profile>, String> {
    let row = sqlx::query(&format!(
        "SELECT {PROFILE_COLUMNS} FROM profiles WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&pool(app).await?)
    .await
    .map_err(|error| error.to_string())?;
    Ok(row.as_ref().map(profile_from_row))
}

pub async fn save(app: &AppHandle, profile: &Profile) -> Result<(), String> {
    let pool = pool(app).await?;
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let position = next_or_existing_position(&mut transaction, &profile.id).await?;
    sqlx::query(
        r#"INSERT INTO profiles (
            id, position, name, portal, username, remember, cafile, client_cert,
            client_key, trusted_fingerprint, gateway, client_os, script, hip, requires_otp
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name, portal = excluded.portal, username = excluded.username,
            remember = excluded.remember, cafile = excluded.cafile,
            client_cert = excluded.client_cert, client_key = excluded.client_key,
            trusted_fingerprint = excluded.trusted_fingerprint, gateway = excluded.gateway,
            client_os = excluded.client_os, script = excluded.script, hip = excluded.hip,
            requires_otp = excluded.requires_otp"#,
    )
    .bind(&profile.id)
    .bind(position)
    .bind(&profile.name)
    .bind(&profile.portal)
    .bind(&profile.username)
    .bind(profile.remember)
    .bind(&profile.cafile)
    .bind(&profile.client_cert)
    .bind(&profile.client_key)
    .bind(&profile.trusted_fingerprint)
    .bind(&profile.gateway)
    .bind(&profile.client_os)
    .bind(&profile.script)
    .bind(profile.hip)
    .bind(profile.requires_otp)
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())
}

async fn next_or_existing_position(
    transaction: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<i64, String> {
    if let Some(row) = sqlx::query("SELECT position FROM profiles WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(row.get("position"));
    }
    let row = sqlx::query("SELECT COALESCE(MAX(position), -1) + 1 AS position FROM profiles")
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| error.to_string())?;
    Ok(row.get("position"))
}

pub async fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM profiles WHERE id = ?")
        .bind(id)
        .execute(&pool(app).await?)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn preference(app: &AppHandle, key: &str) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT value FROM preferences WHERE key = ?")
        .bind(key)
        .fetch_optional(&pool(app).await?)
        .await
        .map_err(|error| error.to_string())
}

pub async fn set_preference(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO preferences (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(&pool(app).await?)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[test]
    fn fresh_migrations_build_only_current_application_tables() {
        tauri::async_runtime::block_on(async {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("in-memory database");
            for migration in migrations() {
                sqlx::raw_sql(migration.sql)
                    .execute(&pool)
                    .await
                    .expect("schema migration");
            }

            let names: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE '_sqlx%' ORDER BY name",
            )
            .fetch_all(&pool)
            .await
            .expect("table list");
            assert_eq!(names, ["preferences", "profiles"]);
        });
    }

    #[test]
    fn upgrading_version_one_drops_totals_and_keeps_application_data() {
        tauri::async_runtime::block_on(async {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("in-memory database");
            let migrations = migrations();
            sqlx::raw_sql(migrations[0].sql)
                .execute(&pool)
                .await
                .expect("version one migration");
            sqlx::query(
                "INSERT INTO profiles (id, position, name, portal, username) VALUES ('profile', 0, 'Work', 'vpn.example.test', 'user')",
            )
            .execute(&pool)
            .await
            .expect("profile fixture");
            sqlx::query("INSERT INTO preferences (key, value) VALUES ('theme', 'dark')")
                .execute(&pool)
                .await
                .expect("preference fixture");
            sqlx::query(
                "INSERT INTO network_totals (profile_id, rx_bytes) VALUES ('profile', '123')",
            )
            .execute(&pool)
            .await
            .expect("network totals fixture");

            sqlx::raw_sql(migrations[1].sql)
                .execute(&pool)
                .await
                .expect("version two migration");

            let names: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE '_sqlx%' ORDER BY name",
            )
            .fetch_all(&pool)
            .await
            .expect("table list");
            assert_eq!(names, ["preferences", "profiles"]);
            let profile_name: String =
                sqlx::query_scalar("SELECT name FROM profiles WHERE id = 'profile'")
                    .fetch_one(&pool)
                    .await
                    .expect("retained profile");
            let theme: String =
                sqlx::query_scalar("SELECT value FROM preferences WHERE key = 'theme'")
                    .fetch_one(&pool)
                    .await
                    .expect("retained preference");
            assert_eq!(profile_name, "Work");
            assert_eq!(theme, "dark");
        });
    }

    #[test]
    fn profile_names_fall_back_to_the_portal_host() {
        assert_eq!(
            default_name("https://vpn.example.test/path/"),
            "vpn.example.test"
        );
        assert_eq!(default_name(""), "New connection");
    }
}
