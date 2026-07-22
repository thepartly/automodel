-- @automodel
--    description: Keyset pagination over users using a two-parameter conditional cursor block
--    expect: multiple
-- @end

SELECT id, name, email, updated_at
FROM public.users
WHERE 1 = 1
#[AND (updated_at, id) > (#{cursor_ua_asc_ts?}, #{cursor_ua_asc_id?})]
ORDER BY updated_at ASC, id ASC
LIMIT #{page_size}
