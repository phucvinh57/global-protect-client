//! SQLite-backed application data. Passwords remain in the native keyring.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
pub struct Migration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
}

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
        },
        Migration {
            version: 2,
            description: "remove network totals",
            sql: "DROP TABLE IF EXISTS network_totals;",
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

pub async fn load(pool: &SqlitePool) -> Result<Vec<Profile>, String> {
    let rows = sqlx::query(&format!(
        "SELECT {PROFILE_COLUMNS} FROM profiles ORDER BY position"
    ))
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(rows.iter().map(profile_from_row).collect())
}

pub async fn find(pool: &SqlitePool, id: &str) -> Result<Option<Profile>, String> {
    let row = sqlx::query(&format!(
        "SELECT {PROFILE_COLUMNS} FROM profiles WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(row.as_ref().map(profile_from_row))
}

pub async fn save(pool: &SqlitePool, profile: &Profile) -> Result<(), String> {
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

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM profiles WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn preference(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT value FROM preferences WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())
}

pub async fn set_preference(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO preferences (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[tokio::test]
    async fn fresh_migrations_build_only_current_application_tables() {
        {
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
        }
    }

    #[tokio::test]
    async fn upgrading_version_one_drops_totals_and_keeps_application_data() {
        {
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
        }
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

/// SQLx computes the same SHA-384 checksums as the desktop SQL plugin from
/// these exact SQL bytes. Never reformat released migration SQL.
pub async fn initialize(pool: &SqlitePool) -> Result<(), String> {
    let migrations = migrations()
        .into_iter()
        .map(|m| {
            sqlx::migrate::Migration::new(
                m.version,
                m.description.into(),
                sqlx::migrate::MigrationType::ReversibleUp,
                m.sql.into(),
                false,
            )
        })
        .collect::<Vec<_>>();
    let migrator = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(migrations),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    migrator.run(pool).await.map_err(|e| e.to_string())
}

pub async fn open(path: &std::path::Path) -> Result<SqlitePool, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| e.to_string())?;
    initialize(&pool).await?;
    Ok(pool)
}

pub fn database_path() -> Result<std::path::PathBuf, String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".config")))
        .ok_or("Neither XDG_CONFIG_HOME nor HOME is set")?;
    if !config.is_absolute() {
        return Err("Configuration directory must be absolute".into());
    }
    Ok(config.join("com.npvinh.gp-client/gp-client.db"))
}

pub async fn resolve(pool: &SqlitePool, name: &str) -> Result<Profile, String> {
    if let Some(profile) = find(pool, name).await? {
        return Ok(profile);
    }
    let matches: Vec<_> = load(pool)
        .await?
        .into_iter()
        .filter(|p| p.name == name)
        .collect();
    match matches.len() {
        0 => Err(format!("No profile named {name:?}")),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(format!(
            "Ambiguous profile {name:?}; use an ID: {}",
            matches
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    async fn memory() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn profile_round_trip_order_resolution_and_preferences() {
        let pool = memory().await;
        initialize(&pool).await.unwrap();
        let mut first = Profile {
            id: "one".into(),
            name: "Work".into(),
            portal: "https://vpn.example.test".into(),
            username: "alice".into(),
            remember: true,
            cafile: Some("ca".into()),
            client_cert: Some("cert".into()),
            client_key: Some("key".into()),
            trusted_fingerprint: Some("pin-sha256:test".into()),
            gateway: Some("gateway".into()),
            client_os: Some("Windows".into()),
            script: Some("script".into()),
            hip: true,
            requires_otp: true,
        };
        save(&pool, &first).await.unwrap();
        let mut second = first.clone();
        second.id = "two".into();
        save(&pool, &second).await.unwrap();
        assert_eq!(
            serde_json::to_value(find(&pool, "one").await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(&first).unwrap()
        );
        let error = resolve(&pool, "Work").await.unwrap_err();
        assert!(error.contains("one") && error.contains("two"));
        assert_eq!(resolve(&pool, "one").await.unwrap().id, "one");
        first.name = "Office".into();
        save(&pool, &first).await.unwrap();
        assert_eq!(
            load(&pool)
                .await
                .unwrap()
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
        assert_eq!(resolve(&pool, "Work").await.unwrap().id, "two");
        set_preference(&pool, "theme", "dark").await.unwrap();
        assert_eq!(
            preference(&pool, "theme").await.unwrap().as_deref(),
            Some("dark")
        );
        delete(&pool, "one").await.unwrap();
        assert!(find(&pool, "one").await.unwrap().is_none());
    }

    fn sqlx_migration(m: &Migration) -> sqlx::migrate::Migration {
        sqlx::migrate::Migration::new(
            m.version,
            m.description.into(),
            sqlx::migrate::MigrationType::ReversibleUp,
            m.sql.into(),
            false,
        )
    }

    #[test]
    fn released_desktop_migration_checksums_are_unchanged() {
        // SHA-384 values captured from the released desktop migration strings.
        let expected = [
            (1, "create application data", "526be64bb8a1bbe60510c2c80a41dcf98d38b0e77f61af4c87beb89236b3455034932db4180b4d4c425c91ca9f0f6ee6"),
            (2, "remove network totals", "5be1532eee6c1250ded74c90a9ad2ba9301afbceb723bf3226e25f65dc9be9446ff88056dc955997959dc2ff14f062e9"),
        ];
        for (migration, (version, description, checksum)) in migrations().iter().zip(expected) {
            let m = sqlx_migration(migration);
            assert_eq!(m.version, version);
            assert_eq!(m.description, description);
            assert_eq!(
                m.checksum
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                checksum
            );
        }
    }

    #[tokio::test]
    async fn desktop_version_one_upgrades_and_both_migrators_can_reopen() {
        let pool = memory().await;
        let old = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(vec![sqlx_migration(&migrations()[0])]),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        old.run(&pool).await.unwrap();
        let p = Profile {
            id: "retained".into(),
            name: "Office".into(),
            ..Default::default()
        };
        save(&pool, &p).await.unwrap();
        set_preference(&pool, "theme", "dark").await.unwrap();
        initialize(&pool).await.unwrap();
        initialize(&pool).await.unwrap();
        let desktop = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(migrations().iter().map(sqlx_migration).collect()),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        desktop.run(&pool).await.unwrap();
        assert_eq!(
            find(&pool, "retained").await.unwrap().unwrap().name,
            "Office"
        );
        assert_eq!(
            preference(&pool, "theme").await.unwrap().as_deref(),
            Some("dark")
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name = 'network_totals'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }
}
