mod common;

use example_app::generated;

/// Regression test: a conditional block containing two input parameters
/// (e.g. `#[AND (updated_at, id) > (#{cursor_ua_asc_ts?}, #{cursor_ua_asc_id?})]`)
/// must renumber and bind BOTH parameters, not just the first one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cursor_block_two_params_included() {
    let pool = common::get_pool().await;

    // Ensure at least a couple of users exist.
    common::insert_test_user(pool, "cursor_a").await;
    common::insert_test_user(pool, "cursor_b").await;

    // First page: no cursor provided, block excluded.
    let first_page = generated::users::get_users_cursor(pool, None, None, 2)
        .await
        .expect("first page (block excluded) should succeed");
    assert_eq!(first_page.len(), 2);

    // Use the last row of the first page as the keyset cursor. Providing both
    // cursor parameters exercises the two-parameter conditional block.
    let last = first_page.last().unwrap();
    let cursor_ts = last.updated_at;
    let cursor_id = last.id;

    let second_page = generated::users::get_users_cursor(pool, cursor_ts, Some(cursor_id), 100)
        .await
        .expect("second page (block included with two params) should succeed");

    // Every returned row must come strictly after the cursor per the keyset predicate.
    for row in &second_page {
        let after = match (row.updated_at, cursor_ts) {
            (Some(ru), Some(cu)) => ru > cu || (ru == cu && row.id > cursor_id),
            _ => true,
        };
        assert!(
            after,
            "row (updated_at={:?}, id={}) should be after cursor (updated_at={:?}, id={})",
            row.updated_at, row.id, cursor_ts, cursor_id
        );
    }
}
