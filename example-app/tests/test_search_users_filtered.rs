mod common;

use example_app::generated;
use example_app::generated::users::{SearchUsersFilteredItem, SearchUsersFilteredSort};

/// The optional filter/cursor parameters of `search_users_filtered`, defaulted
/// so each test only sets the fields it cares about. The `sort` enum is passed
/// explicitly to `run`; sorted variants carry their own `limit` field, while
/// the `Unsorted` variant uses the query's hardcoded `LIMIT 100`.
#[derive(Default)]
struct Search {
    min_id: i32,
    name_exact: Option<String>,
    name_starts_with: Option<String>,
    email_exact: Option<String>,
    age_from: Option<i32>,
    age_to: Option<i32>,
    is_active: Option<bool>,
    created_from: Option<chrono::DateTime<chrono::Utc>>,
    created_to: Option<chrono::DateTime<chrono::Utc>>,
    cursor_ua_asc_ts: Option<chrono::DateTime<chrono::Utc>>,
    cursor_ua_asc_id: Option<i32>,
    cursor_ua_desc_ts: Option<chrono::DateTime<chrono::Utc>>,
    cursor_ua_desc_id: Option<i32>,
    cursor_name_asc_val: Option<String>,
    cursor_name_asc_id: Option<i32>,
    cursor_name_desc_val: Option<String>,
    cursor_name_desc_id: Option<i32>,
}

async fn run(
    pool: &sqlx::PgPool,
    s: Search,
    sort: SearchUsersFilteredSort,
) -> Vec<SearchUsersFilteredItem> {
    generated::users::search_users_filtered(
        pool,
        s.min_id,
        s.name_exact,
        s.name_starts_with,
        s.email_exact,
        s.age_from,
        s.age_to,
        s.is_active,
        s.created_from,
        s.created_to,
        s.cursor_ua_asc_ts,
        s.cursor_ua_asc_id,
        s.cursor_ua_desc_ts,
        s.cursor_ua_desc_id,
        s.cursor_name_asc_val,
        s.cursor_name_asc_id,
        s.cursor_name_desc_val,
        s.cursor_name_desc_id,
        sort,
    )
    .await
    .expect("search_users_filtered failed")
}

/// Insert three isolated users whose names sort a < b < c, ages 21 < 22 < 23.
/// Returns (like_prefix, [inserted rows]).
async fn seed(pool: &sqlx::PgPool) -> (String, Vec<generated::users::InsertUserItem>) {
    // Lowercase letters + digits only: safe as a LIKE prefix (no % or _), and
    // deterministically sortable by name. A process-wide atomic counter guarantees
    // uniqueness even when tests run concurrently in the same nanosecond.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Fixed-width fields so no tag can ever be a LIKE-prefix of another tag.
    let tag = format!("zzsf{:019}x{:06}", ts, n);

    let mut rows = Vec::new();
    for (suffix, age) in [("a", 21), ("b", 22), ("c", 23)] {
        let name = format!("{}{}", tag, suffix);
        // Derive the email from the unique tag so it can never collide.
        let email = format!("{}.{}@test.example.com", tag, suffix);
        let row = generated::users::insert_user(pool, name, email, age, common::default_profile())
            .await
            .expect("insert failed");
        rows.push(row);
    }
    (format!("{}%", tag), rows)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn where_only_unsorted_returns_all_matches() {
    let pool = common::get_pool().await;
    let (prefix, _rows) = seed(pool).await;

    let results = run(
        pool,
        Search {
            min_id: 0,
            name_starts_with: Some(prefix),
            ..Default::default()
        },
        SearchUsersFilteredSort::Unsorted,
    )
    .await;

    assert_eq!(results.len(), 3, "expected exactly the three seeded users");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_where_filters_are_and_combined() {
    let pool = common::get_pool().await;
    let (prefix, _rows) = seed(pool).await;

    // age_from + age_to narrow to the single middle user (age 22).
    let results = run(
        pool,
        Search {
            min_id: 0,
            name_starts_with: Some(prefix),
            age_from: Some(22),
            age_to: Some(22),
            ..Default::default()
        },
        SearchUsersFilteredSort::Unsorted,
    )
    .await;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].age, Some(22));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn where_plus_order_by_name_asc_is_sorted_and_limited() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    let results = run(
        pool,
        Search {
            min_id: 0,
            name_starts_with: Some(prefix),
            ..Default::default()
        },
        SearchUsersFilteredSort::NameAsc { limit: 2 },
    )
    .await;

    assert_eq!(results.len(), 2, "limit should cap at 2 rows");
    assert_eq!(results[0].name, rows[0].name); // ...a
    assert_eq!(results[1].name, rows[1].name); // ...b
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn where_plus_order_by_name_desc() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    let results = run(
        pool,
        Search {
            min_id: 0,
            name_starts_with: Some(prefix),
            ..Default::default()
        },
        SearchUsersFilteredSort::NameDesc { limit: 2 },
    )
    .await;

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, rows[2].name); // ...c
    assert_eq!(results[1].name, rows[1].name); // ...b
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn where_plus_cursor_keyset_pagination() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // Page 1: first two by name asc.
    let page1 = run(
        pool,
        Search {
            min_id: 0,
            name_starts_with: Some(prefix.clone()),
            ..Default::default()
        },
        SearchUsersFilteredSort::NameAsc { limit: 2 },
    )
    .await;
    assert_eq!(page1.len(), 2);
    let last = page1.last().unwrap();

    // Page 2: keyset cursor continues from the last row of page 1.
    let page2 = run(
        pool,
        Search {
            min_id: 0,
            name_starts_with: Some(prefix),
            cursor_name_asc_val: Some(last.name.clone()),
            cursor_name_asc_id: Some(last.id),
            ..Default::default()
        },
        SearchUsersFilteredSort::NameAsc { limit: 2 },
    )
    .await;

    assert_eq!(page2.len(), 1, "only the third seeded user remains");
    assert_eq!(page2[0].name, rows[2].name); // ...c
}
