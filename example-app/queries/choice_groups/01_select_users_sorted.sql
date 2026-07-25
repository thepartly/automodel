-- @automodel
--   description: Pure choice group over users — the caller picks exactly one sort direction; the page size is referenced in every branch so each enum variant carries its own `page` field
--   expect: multiple
-- @end
SELECT id, name, email
FROM public.users
WHERE email LIKE #{email_prefix}
#[#{sort=asc} ORDER BY id ASC LIMIT #{page?}]
#[#{sort=desc} ORDER BY id DESC LIMIT #{page?}]
