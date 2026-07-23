mod common;

use example_app::generated;
use example_app::generated::choice_groups::{
    CursorOptionalFirstPageSort, DirectAndNestedMixedFilter, DualNestedAgeBoundsSort,
    MultiGroupSearchRange, MultiGroupSearchSort, SearchUsersMixedSort,
    SelectUsersOptionalSortOrder, SelectUsersSortedSort,
};

/// Insert three isolated users whose names sort a < b < c, ages 21 < 22 < 23,
/// and whose emails all share a unique LIKE-safe prefix. Returns
/// `(like_prefix, [inserted rows in a/b/c order])`.
///
/// A process-wide atomic counter combined with a nanosecond timestamp keeps the
/// prefix unique even when tests run concurrently, and the fixed-width layout
/// guarantees no tag can ever be a LIKE-prefix of another.
async fn seed(pool: &sqlx::PgPool) -> (String, Vec<generated::users::InsertUserItem>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tag = format!("zzcg{:019}x{:06}", ts, n);

    let mut rows = Vec::new();
    for (suffix, age) in [("a", 21), ("b", 22), ("c", 23)] {
        let name = format!("{}{}", tag, suffix);
        let email = format!("{}.{}@test.example.com", tag, suffix);
        let row = generated::users::insert_user(pool, name, email, age, common::default_profile())
            .await
            .expect("insert failed");
        rows.push(row);
    }
    (format!("{}%", tag), rows)
}

/// Pure required choice group: the caller must pick exactly one sort direction,
/// and `page` is referenced in every branch so each enum variant carries its own
/// `page` field.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pure_choice_group_with_shared_page() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // Ascending by id == insertion order (a, b, c); page caps the result at 2.
    let asc = generated::choice_groups::select_users_sorted(
        pool,
        prefix.clone(),
        SelectUsersSortedSort::Asc { page: 2 },
    )
    .await
    .expect("asc select failed");
    assert_eq!(asc.len(), 2, "page field should cap at 2 rows");
    assert_eq!(asc[0].id, rows[0].id);
    assert_eq!(asc[1].id, rows[1].id);

    // Descending by id (c, b, a); page caps at 2.
    let desc = generated::choice_groups::select_users_sorted(
        pool,
        prefix,
        SelectUsersSortedSort::Desc { page: 2 },
    )
    .await
    .expect("desc select failed");
    assert_eq!(desc.len(), 2);
    assert_eq!(desc[0].id, rows[2].id);
    assert_eq!(desc[1].id, rows[1].id);
}

/// Optional choice group: `None` selects the base query (no ORDER BY), while
/// each `Some(variant)` applies exactly one ordering.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_choice_group_base_and_variants() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // None -> base query: all three seeded users, order unspecified.
    let base = generated::choice_groups::select_users_optional_sort(pool, prefix.clone(), None)
        .await
        .expect("base select failed");
    assert_eq!(base.len(), 3);

    // Some(ByName) -> names sort a < b < c.
    let by_name = generated::choice_groups::select_users_optional_sort(
        pool,
        prefix,
        Some(SelectUsersOptionalSortOrder::ByName),
    )
    .await
    .expect("by_name select failed");
    assert_eq!(by_name.len(), 3);
    assert_eq!(by_name[0].name, rows[0].name);
    assert_eq!(by_name[1].name, rows[1].name);
    assert_eq!(by_name[2].name, rows[2].name);
}

/// Mixing additive `#[...]` filters with a choice group: the additive age
/// bounds combine freely, the `Unsorted` branch is a parameterless unit variant
/// (hardcoded `LIMIT 100`), and the sorted branches carry a per-variant `limit`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_additive_filters_and_choice_group() {
    let pool = common::get_pool().await;
    let (prefix, _rows) = seed(pool).await;

    // Unsorted, no additive filters: the paramless branch returns all three
    // (hardcoded LIMIT 100 comfortably covers the seeded rows).
    let unsorted = generated::choice_groups::search_users_mixed(
        pool,
        prefix.clone(),
        None,
        None,
        SearchUsersMixedSort::Unsorted,
    )
    .await
    .expect("unsorted select failed");
    assert_eq!(unsorted.len(), 3);

    // AgeAsc with an additive lower bound: ages >= 22 -> b(22), c(23) ascending.
    let asc = generated::choice_groups::search_users_mixed(
        pool,
        prefix.clone(),
        Some(22),
        None,
        SearchUsersMixedSort::AgeAsc { limit: 5 },
    )
    .await
    .expect("age_asc select failed");
    assert_eq!(asc.len(), 2);
    assert_eq!(asc[0].age, Some(22));
    assert_eq!(asc[1].age, Some(23));

    // AgeDesc with both additive bounds: ages in [21, 22] -> b(22), a(21).
    let desc = generated::choice_groups::search_users_mixed(
        pool,
        prefix,
        Some(21),
        Some(22),
        SearchUsersMixedSort::AgeDesc { limit: 5 },
    )
    .await
    .expect("age_desc select failed");
    assert_eq!(desc.len(), 2);
    assert_eq!(desc[0].age, Some(22));
    assert_eq!(desc[1].age, Some(21));
}

/// Two independent choice groups in a single query: an optional `range` group
/// (per-variant field on each branch) and a required `sort` group whose `lim`
/// argument is carried as a per-variant field in both branches. Each group
/// compiles to its own enum
/// argument and the two selectors are chosen independently.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_independent_choice_groups() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // range=None, sort=Asc, lim=2: no age filter, ascending by id, capped at 2.
    let none_asc = generated::choice_groups::multi_group_search(
        pool,
        prefix.clone(),
        None,
        MultiGroupSearchSort::Asc { lim: 2 },
    )
    .await
    .expect("none/asc select failed");
    assert_eq!(none_asc.len(), 2, "lim field should cap at 2 rows");
    assert_eq!(none_asc[0].id, rows[0].id);
    assert_eq!(none_asc[1].id, rows[1].id);

    // range=Min{22}, sort=Asc: ages >= 22 -> b(22), c(23) ascending.
    let min_asc = generated::choice_groups::multi_group_search(
        pool,
        prefix.clone(),
        Some(MultiGroupSearchRange::Min { min_age: 22 }),
        MultiGroupSearchSort::Asc { lim: 10 },
    )
    .await
    .expect("min/asc select failed");
    assert_eq!(min_asc.len(), 2);
    assert_eq!(min_asc[0].age, Some(22));
    assert_eq!(min_asc[1].age, Some(23));

    // range=Max{22}, sort=Desc: ages <= 22 -> a(21), b(22), descending id -> b, a.
    let max_desc = generated::choice_groups::multi_group_search(
        pool,
        prefix,
        Some(MultiGroupSearchRange::Max { max_age: 22 }),
        MultiGroupSearchSort::Desc { lim: 10 },
    )
    .await
    .expect("max/desc select failed");
    assert_eq!(max_desc.len(), 2);
    assert_eq!(max_desc[0].id, rows[1].id);
    assert_eq!(max_desc[1].id, rows[0].id);
}

/// Option B: keyset pagination where the cursor predicate is a nested optional
/// block inside each sort variant. When the cursor fields are `None` the query
/// returns the first page (no keyset predicate); when they are `Some` the query
/// applies the keyset predicate to return the following page. Seeded rows sort
/// a < b < c by name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursor_optional_first_page_paginates() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;
    let (a, b, c) = (&rows[0], &rows[1], &rows[2]);

    // First page, ascending by name, no cursor: None -> no keyset predicate.
    let page1 = generated::choice_groups::cursor_optional_first_page(
        pool,
        prefix.clone(),
        2,
        Some(CursorOptionalFirstPageSort::NameAsc {
            cur_name_asc_val: None,
            cur_name_asc_id: None,
        }),
    )
    .await
    .expect("name_asc first page failed");
    assert_eq!(page1.len(), 2, "lim should cap the first page at 2 rows");
    assert_eq!(page1[0].id, a.id);
    assert_eq!(page1[1].id, b.id);

    // Next page, using the last row of page 1 as the cursor: Some -> keyset
    // predicate `(name, id) > (b.name, b.id)` -> only c remains.
    let page2 = generated::choice_groups::cursor_optional_first_page(
        pool,
        prefix.clone(),
        2,
        Some(CursorOptionalFirstPageSort::NameAsc {
            cur_name_asc_val: Some(b.name.clone()),
            cur_name_asc_id: Some(b.id),
        }),
    )
    .await
    .expect("name_asc next page failed");
    assert_eq!(page2.len(), 1, "only one row should remain after the cursor");
    assert_eq!(page2[0].id, c.id);

    // Descending first page, no cursor: c, b, a -> capped at 2 -> c, b.
    let desc = generated::choice_groups::cursor_optional_first_page(
        pool,
        prefix.clone(),
        2,
        Some(CursorOptionalFirstPageSort::NameDesc {
            cur_name_desc_val: None,
            cur_name_desc_id: None,
        }),
    )
    .await
    .expect("name_desc first page failed");
    assert_eq!(desc.len(), 2);
    assert_eq!(desc[0].id, c.id);
    assert_eq!(desc[1].id, b.id);

    // Descending next page from c: `(name, id) < (c.name, c.id)` -> b, a.
    let desc2 = generated::choice_groups::cursor_optional_first_page(
        pool,
        prefix,
        10,
        Some(CursorOptionalFirstPageSort::NameDesc {
            cur_name_desc_val: Some(c.name.clone()),
            cur_name_desc_id: Some(c.id),
        }),
    )
    .await
    .expect("name_desc next page failed");
    assert_eq!(desc2.len(), 2);
    assert_eq!(desc2[0].id, b.id);
    assert_eq!(desc2[1].id, a.id);
}

/// Multiple nested optional blocks inside a single required variant: each `sort`
/// branch carries an optional lower AND upper age bound as independent nested
/// blocks. Seeded rows have ages 21 (a), 22 (b), 23 (c).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dual_nested_age_bounds_combinations() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;
    let (a, b, c) = (&rows[0], &rows[1], &rows[2]);

    // No bounds: all three ascending by age.
    let all_asc = generated::choice_groups::dual_nested_age_bounds(
        pool,
        prefix.clone(),
        10,
        DualNestedAgeBoundsSort::Asc {
            asc_min_age: None,
            asc_max_age: None,
        },
    )
    .await
    .expect("asc no-bounds failed");
    assert_eq!(
        all_asc.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![a.id, b.id, c.id]
    );

    // Lower bound only: age >= 22 -> b, c.
    let min_only = generated::choice_groups::dual_nested_age_bounds(
        pool,
        prefix.clone(),
        10,
        DualNestedAgeBoundsSort::Asc {
            asc_min_age: Some(22),
            asc_max_age: None,
        },
    )
    .await
    .expect("asc min-only failed");
    assert_eq!(min_only.iter().map(|r| r.age).collect::<Vec<_>>(), vec![Some(22), Some(23)]);

    // Both bounds: 22 <= age <= 22 -> only b.
    let both = generated::choice_groups::dual_nested_age_bounds(
        pool,
        prefix.clone(),
        10,
        DualNestedAgeBoundsSort::Asc {
            asc_min_age: Some(22),
            asc_max_age: Some(22),
        },
    )
    .await
    .expect("asc both-bounds failed");
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].id, b.id);

    // Descending with upper bound only: age <= 22 -> b, a (desc by age).
    let desc_max = generated::choice_groups::dual_nested_age_bounds(
        pool,
        prefix,
        10,
        DualNestedAgeBoundsSort::Desc {
            desc_min_age: None,
            desc_max_age: Some(22),
        },
    )
    .await
    .expect("desc max-only failed");
    assert_eq!(
        desc_max.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![b.id, a.id]
    );
}

/// A mandatory direct parameter and an optional nested block coexist in the same
/// variant. `by_active` always binds `want_active` (plain `bool`) and may narrow
/// by a nested minimum age; `by_age` always binds `floor_age` (plain `i32`) and
/// may cap with a nested maximum age.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_and_nested_mixed_variants() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;
    let (a, b, c) = (&rows[0], &rows[1], &rows[2]);

    // by_active with no nested age floor: seeded users are all active -> a, b, c.
    let active_all = generated::choice_groups::direct_and_nested_mixed(
        pool,
        prefix.clone(),
        10,
        DirectAndNestedMixedFilter::ByActive {
            want_active: true,
            active_min_age: None,
        },
    )
    .await
    .expect("by_active no-nested failed");
    assert_eq!(
        active_all.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![a.id, b.id, c.id]
    );
    assert!(active_all.iter().all(|r| r.is_active == Some(true)));

    // by_active with nested age floor 23 -> only c.
    let active_old = generated::choice_groups::direct_and_nested_mixed(
        pool,
        prefix.clone(),
        10,
        DirectAndNestedMixedFilter::ByActive {
            want_active: true,
            active_min_age: Some(23),
        },
    )
    .await
    .expect("by_active nested-floor failed");
    assert_eq!(active_old.len(), 1);
    assert_eq!(active_old[0].id, c.id);

    // by_age direct floor 22, no nested ceiling -> b, c.
    let age_floor = generated::choice_groups::direct_and_nested_mixed(
        pool,
        prefix.clone(),
        10,
        DirectAndNestedMixedFilter::ByAge {
            floor_age: 22,
            ceil_age: None,
        },
    )
    .await
    .expect("by_age floor-only failed");
    assert_eq!(
        age_floor.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![b.id, c.id]
    );

    // by_age direct floor 22 with nested ceiling 22 -> only b.
    let age_band = generated::choice_groups::direct_and_nested_mixed(
        pool,
        prefix,
        10,
        DirectAndNestedMixedFilter::ByAge {
            floor_age: 22,
            ceil_age: Some(22),
        },
    )
    .await
    .expect("by_age floor+ceiling failed");
    assert_eq!(age_band.len(), 1);
    assert_eq!(age_band[0].id, b.id);
}
