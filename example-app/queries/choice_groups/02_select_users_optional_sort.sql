-- @automodel
--   description: Optional choice group over users — omit the selector to keep the default row order, or pick exactly one ordering. Because the marker is `?`, the generated selector argument is an Option and None selects the base query
--   expect: multiple
-- @end
SELECT id, name, email
FROM public.users
WHERE email LIKE #{email_prefix}
#[#{order=by_name?} ORDER BY name ASC]
#[#{order=by_email?} ORDER BY email ASC]
