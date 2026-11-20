//! Desktop adapters for the shared application store.
pub use gp_store::settings::{default_name, new_id, Profile, DATABASE_URL};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tauri_plugin_sql::{DbInstances, DbPool, Migration, MigrationKind};

pub fn migrations() -> Vec<Migration> {
    gp_store::settings::migrations()
        .into_iter()
        .map(|m| Migration {
            version: m.version,
            description: m.description,
            sql: m.sql,
            kind: MigrationKind::Up,
        })
        .collect()
}
async fn pool(app: &AppHandle) -> Result<SqlitePool, String> {
    let instances = app.state::<DbInstances>();
    let instances = instances.0.read().await;
    match instances.get(DATABASE_URL) {
        Some(DbPool::Sqlite(pool)) => Ok(pool.clone()),
        _ => Err("Application database is not available".into()),
    }
}
pub async fn load(app: &AppHandle) -> Result<Vec<Profile>, String> {
    gp_store::settings::load(&pool(app).await?).await
}
pub async fn find(app: &AppHandle, id: &str) -> Result<Option<Profile>, String> {
    gp_store::settings::find(&pool(app).await?, id).await
}
pub async fn save(app: &AppHandle, profile: &Profile) -> Result<(), String> {
    gp_store::settings::save(&pool(app).await?, profile).await
}
pub async fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    gp_store::settings::delete(&pool(app).await?, id).await
}
pub async fn preference(app: &AppHandle, key: &str) -> Result<Option<String>, String> {
    gp_store::settings::preference(&pool(app).await?, key).await
}
pub async fn set_preference(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    gp_store::settings::set_preference(&pool(app).await?, key, value).await
}
