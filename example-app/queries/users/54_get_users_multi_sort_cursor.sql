-- @automodel
--    description: Keyset pagination over users with mutually-exclusive sort modes
--    expect: multiple
-- @end
SELECT id, name, email, updated_at
FROM public.users
WHERE 1 = 1
#[#{sort=ua_asc!} AND (updated_at, id) > (#{cursor_ts?}, #{cursor_id?}) ORDER BY updated_at ASC, id ASC LIMIT #{page_size?}]
#[#{sort=ua_desc!} AND (updated_at, id) < (#{cursor_ts?}, #{cursor_id?}) ORDER BY updated_at DESC, id DESC LIMIT #{page_size?}]
#[#{sort=name_asc!} ORDER BY name ASC, id ASC LIMIT #{page_size?}]
#[#{sort=name_desc!} ORDER BY name DESC, id DESC LIMIT #{page_size?}]

