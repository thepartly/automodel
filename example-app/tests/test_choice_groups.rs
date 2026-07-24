mod common;

use example_app::generated;
use example_app::generated::choice_groups::{
    CursorOptionalFirstPageSort, DirectAndNestedMixedFilter, DualNestedAgeBoundsSort,
    MultiGroupSearchRange, MultiGroupSearchSort, SearchUsersMixedSort,
    SelectUsersOptionalSortOrder, SelectUsersSortedSort, UserOptionalOwnFieldAge,
    UserOptionalPostsPosts, UserOptionalReferrerAndPostsPosts,
    UserOptionalReferrerAndPostsReferrer, UserOptionalReferrerFullReferrer,
    UserOptionalReferrerReferrer,
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
    assert_eq!(
        page2.len(),
        1,
        "only one row should remain after the cursor"
    );
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
    assert_eq!(
        min_only.iter().map(|r| r.age).collect::<Vec<_>>(),
        vec![Some(22), Some(23)]
    );

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

/// Coordinated choice group: a single selector drives two fragments that must
/// switch together — the projection (`r.age` vs literal `NULL`) and the matching
/// `LEFT JOIN`. `Off` skips the join entirely and returns `NULL` for every row;
/// `On` runs the join and surfaces the referrer's age. The result shape is fixed
/// (`referrer_age` is always present as `Option<i32>`), so no row-mapping changes
/// are needed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn coordinated_optional_join_and_projection() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // Make the middle user (b, age 22) refer to the first user (a, age 21).
    sqlx::query("UPDATE public.users SET referrer_id = $1 WHERE id = $2")
        .bind(rows[0].id)
        .bind(rows[1].id)
        .execute(pool)
        .await
        .expect("set referrer failed");

    // Off: the LEFT JOIN is dropped from the SQL and referrer_age is NULL for all.
    let off = generated::choice_groups::user_optional_referrer(
        pool,
        prefix.clone(),
        UserOptionalReferrerReferrer::Off,
    )
    .await
    .expect("off select failed");
    assert_eq!(off.len(), 3);
    assert!(off.iter().all(|r| r.referrer_age.is_none()));

    // On: the join runs; rows are ordered by u.id (a, b, c). Only b has a
    // referrer, so it carries a's age (21); a and c have no referrer -> None.
    let on = generated::choice_groups::user_optional_referrer(
        pool,
        prefix,
        UserOptionalReferrerReferrer::On,
    )
    .await
    .expect("on select failed");
    assert_eq!(on.len(), 3);
    assert_eq!(on[0].referrer_age, None);
    assert_eq!(on[1].referrer_age, Some(21));
    assert_eq!(on[2].referrer_age, None);
}

/// Conditional NON-joined projection: the selector flips a single base-table
/// column (`u.age`) on or off without any join. `On` returns each user's own age;
/// `Off` returns `NULL`. Each branch is exactly one block, so this rides the
/// isolated-variant (clean) generator rather than the membership-based one. The
/// result shape stays fixed (`maybe_age: Option<i32>`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_non_joined_field() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // Off: every row's maybe_age is NULL.
    let off = generated::choice_groups::user_optional_own_field(
        pool,
        prefix.clone(),
        UserOptionalOwnFieldAge::Off,
    )
    .await
    .expect("off select failed");
    assert_eq!(off.len(), 3);
    assert!(off.iter().all(|r| r.maybe_age.is_none()));

    // On: rows are ordered by u.id (a, b, c) with ages 21, 22, 23.
    let on = generated::choice_groups::user_optional_own_field(
        pool,
        prefix,
        UserOptionalOwnFieldAge::On,
    )
    .await
    .expect("on select failed");
    assert_eq!(on.len(), 3);
    assert_eq!(on[0].id, rows[0].id);
    assert_eq!(on[0].maybe_age, Some(21));
    assert_eq!(on[1].maybe_age, Some(22));
    assert_eq!(on[2].maybe_age, Some(23));
}

/// Conditional WHOLE-entity projection: instead of one column, the selector
/// returns the entire referrer row as a nested composite (`Option<..::Users>`).
/// `On` adds the self LEFT JOIN and hydrates the full referrer struct; `Off`
/// drops the join and returns `None`. No JSON is involved — this is the native
/// Postgres row type decoded straight into the generated `Users` composite.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_whole_referrer_composite() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // Make the middle user (b) refer to the first user (a).
    sqlx::query("UPDATE public.users SET referrer_id = $1 WHERE id = $2")
        .bind(rows[0].id)
        .bind(rows[1].id)
        .execute(pool)
        .await
        .expect("set referrer failed");

    // Off: the join is gone; every referrer is None.
    let off = generated::choice_groups::user_optional_referrer_full(
        pool,
        prefix.clone(),
        UserOptionalReferrerFullReferrer::Off,
    )
    .await
    .expect("off select failed");
    assert_eq!(off.len(), 3);
    assert!(off.iter().all(|r| r.referrer.is_none()));

    // On: only b has a referrer, and it carries a's full row (id, name, age).
    let on = generated::choice_groups::user_optional_referrer_full(
        pool,
        prefix,
        UserOptionalReferrerFullReferrer::On,
    )
    .await
    .expect("on select failed");
    assert_eq!(on.len(), 3);
    assert!(on[0].referrer.is_none());
    assert!(on[2].referrer.is_none());
    let referrer = on[1].referrer.as_ref().expect("b should have a referrer");
    assert_eq!(referrer.id, rows[0].id);
    assert_eq!(referrer.name, rows[0].name);
    assert_eq!(referrer.age, Some(21));
}

/// Conditional CHILD COLLECTION without a JSON aggregate: the selector drives a
/// three-fragment coordinated branch (projection + LEFT JOIN + GROUP BY). `On`
/// builds `array_agg(p)` over the child table's implicit composite type, decoding
/// straight into `Vec<..::Posts>`; `Off` drops all three fragments and returns
/// `None`. Proves a collection of children can be returned natively, no JSON.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_child_collection_composite_array() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // Give the first user (a) two posts and leave b and c without any.
    for title in ["First post", "Second post"] {
        sqlx::query("INSERT INTO public.posts (author_id, title) VALUES ($1, $2)")
            .bind(rows[0].id)
            .bind(title)
            .execute(pool)
            .await
            .expect("insert post failed");
    }

    // Off: the join, aggregate and GROUP BY all vanish; posts is None everywhere.
    let off = generated::choice_groups::user_optional_posts(
        pool,
        prefix.clone(),
        UserOptionalPostsPosts::Off,
    )
    .await
    .expect("off select failed");
    assert_eq!(off.len(), 3);
    assert!(off.iter().all(|r| r.posts.is_none()));

    // On: a carries a Vec of its two posts; b and c have no posts so the
    // FILTER'd array_agg yields NULL -> None.
    let on =
        generated::choice_groups::user_optional_posts(pool, prefix, UserOptionalPostsPosts::On)
            .await
            .expect("on select failed");
    assert_eq!(on.len(), 3);
    assert_eq!(on[0].id, rows[0].id);
    let posts = on[0].posts.as_ref().expect("a should have posts");
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].author_id, rows[0].id);
    assert_eq!(posts[0].title, "First post");
    assert_eq!(posts[1].title, "Second post");
    assert!(on[1].posts.is_none());
    assert!(on[2].posts.is_none());
}

/// TWO independent selectors in one query: `referrer` (whole-row composite via a
/// self LEFT JOIN) and `posts` (child collection via a correlated `array_agg`
/// subquery). The selectors are orthogonal, so all four On/Off combinations must
/// produce a valid fixed-shape row with each field hydrated or `None`
/// independently of the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_selectors_referrer_and_posts() {
    let pool = common::get_pool().await;
    let (prefix, rows) = seed(pool).await;

    // b refers to a; a owns two posts. b and c have neither referrer nor posts.
    sqlx::query("UPDATE public.users SET referrer_id = $1 WHERE id = $2")
        .bind(rows[0].id)
        .bind(rows[1].id)
        .execute(pool)
        .await
        .expect("set referrer failed");
    for title in ["First post", "Second post"] {
        sqlx::query("INSERT INTO public.posts (author_id, title) VALUES ($1, $2)")
            .bind(rows[0].id)
            .bind(title)
            .execute(pool)
            .await
            .expect("insert post failed");
    }

    // Off/Off: both columns are NULL for every row.
    let off_off = generated::choice_groups::user_optional_referrer_and_posts(
        pool,
        prefix.clone(),
        UserOptionalReferrerAndPostsReferrer::Off,
        UserOptionalReferrerAndPostsPosts::Off,
    )
    .await
    .expect("off/off select failed");
    assert_eq!(off_off.len(), 3);
    assert!(off_off
        .iter()
        .all(|r| r.referrer.is_none() && r.posts.is_none()));

    // On/Off: referrer hydrated (only b), posts always None.
    let on_off = generated::choice_groups::user_optional_referrer_and_posts(
        pool,
        prefix.clone(),
        UserOptionalReferrerAndPostsReferrer::On,
        UserOptionalReferrerAndPostsPosts::Off,
    )
    .await
    .expect("on/off select failed");
    assert_eq!(on_off.len(), 3);
    assert!(on_off.iter().all(|r| r.posts.is_none()));
    assert!(on_off[0].referrer.is_none());
    assert_eq!(on_off[1].referrer.as_ref().map(|r| r.id), Some(rows[0].id));
    assert!(on_off[2].referrer.is_none());

    // Off/On: posts hydrated (only a), referrer always None.
    let off_on = generated::choice_groups::user_optional_referrer_and_posts(
        pool,
        prefix.clone(),
        UserOptionalReferrerAndPostsReferrer::Off,
        UserOptionalReferrerAndPostsPosts::On,
    )
    .await
    .expect("off/on select failed");
    assert_eq!(off_on.len(), 3);
    assert!(off_on.iter().all(|r| r.referrer.is_none()));
    assert_eq!(off_on[0].posts.as_ref().map(|p| p.len()), Some(2));
    assert!(off_on[1].posts.is_none());
    assert!(off_on[2].posts.is_none());

    // On/On: both selectors active and independent.
    let on_on = generated::choice_groups::user_optional_referrer_and_posts(
        pool,
        prefix,
        UserOptionalReferrerAndPostsReferrer::On,
        UserOptionalReferrerAndPostsPosts::On,
    )
    .await
    .expect("on/on select failed");
    assert_eq!(on_on.len(), 3);
    // a: no referrer, two posts.
    assert!(on_on[0].referrer.is_none());
    let a_posts = on_on[0].posts.as_ref().expect("a should have posts");
    assert_eq!(a_posts.len(), 2);
    assert_eq!(a_posts[0].title, "First post");
    // b: referrer is a, no posts.
    let b_referrer = on_on[1]
        .referrer
        .as_ref()
        .expect("b should have a referrer");
    assert_eq!(b_referrer.id, rows[0].id);
    assert_eq!(b_referrer.age, Some(21));
    assert!(on_on[1].posts.is_none());
    // c: neither.
    assert!(on_on[2].referrer.is_none());
    assert!(on_on[2].posts.is_none());
}
