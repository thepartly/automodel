use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn get_pool() -> &'static PgPool {
    let url = std::env::var("AUTOMODEL_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@automodel-testdb:5432/postgres".into());
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&url)
        .await
        .expect("Failed to connect to database");
    Box::leak(Box::new(pool))
}

/// Helper to generate unique emails for test isolation
pub fn unique_email(prefix: &str) -> String {
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{}.{}@test.example.com", prefix, ts)
}

/// Insert a test user and return its id — convenience for tests that need a user to exist
pub async fn insert_test_user(
    pool: &PgPool,
    prefix: &str,
) -> example_app::generated::users::InsertUserItem {
    let email = unique_email(prefix);
    example_app::generated::users::insert_user(
        pool,
        format!("Test {}", prefix),
        email,
        30,
        example_app::models::UserProfile {
            bio: None,
            avatar_url: None,
            preferences: example_app::models::UserPreferences {
                theme: "dark".to_string(),
                language: "en".to_string(),
                notifications_enabled: true,
            },
            social_links: vec![],
        },
    )
    .await
    .expect("insert_test_user failed")
}

pub fn default_profile() -> example_app::models::UserProfile {
    example_app::models::UserProfile {
        bio: None,
        avatar_url: None,
        preferences: example_app::models::UserPreferences {
            theme: "dark".to_string(),
            language: "en".to_string(),
            notifications_enabled: true,
        },
        social_links: vec![],
    }
}
