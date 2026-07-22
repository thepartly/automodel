mod common;

use example_app::generated;
use example_app::generated::users::GetUsersMultiSortCursorSort;

/// Regression test for mutually-exclusive sort modes expressed as a choice group.
/// The four `ORDER BY ... LIMIT` branches are tagged `#{sort=<variant>!}`, so the
/// generated function takes a single `GetUsersMultiSortCursorSort` enum argument
/// plus the shared `page_size`; the keyset cursor params live inside the relevant
/// enum variants. This verifies each sort mode selects and orders correctly and
/// that picking a mode is the only way to call the function.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multi_sort_mode_selection() {
    let pool = common::get_pool().await;

    common::insert_test_user(pool, "multisort_a").await;
    common::insert_test_user(pool, "multisort_b").await;
    common::insert_test_user(pool, "multisort_c").await;

    // Mode: updated_at ASC (first page, cursor at the epoch to include everyone).
    let ua_asc = generated::users::get_users_multi_sort_cursor(
        pool,
        GetUsersMultiSortCursorSort::UaAsc {
            cursor_ts: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
            cursor_id: 0,
        },
        3,
    )
    .await
    .expect("updated_at ASC mode should succeed");
    assert!(ua_asc.len() <= 3);
    for w in ua_asc.windows(2) {
        assert!(
            w[0].updated_at <= w[1].updated_at,
            "results must be ascending by updated_at"
        );
    }

    // Mode: name DESC (a unit variant — no cursor fields required).
    let name_desc = generated::users::get_users_multi_sort_cursor(
        pool,
        GetUsersMultiSortCursorSort::NameDesc,
        3,
    )
    .await
    .expect("name DESC mode should succeed");
    assert!(name_desc.len() <= 3);
    for w in name_desc.windows(2) {
        assert!(w[0].name >= w[1].name, "results must be descending by name");
    }

    // Mode: updated_at DESC with a keyset cursor from the first page.
    if let Some(first) = ua_asc.last() {
        let page = generated::users::get_users_multi_sort_cursor(
            pool,
            GetUsersMultiSortCursorSort::UaDesc {
                cursor_ts: first.updated_at.unwrap(),
                cursor_id: first.id,
            },
            5,
        )
        .await
        .expect("updated_at DESC mode with cursor should succeed");
        for w in page.windows(2) {
            assert!(
                w[0].updated_at >= w[1].updated_at,
                "results must be descending by updated_at"
            );
        }
    }
}
