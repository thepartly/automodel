-- @automodel
--   description: Conditionally return a COLLECTION of child rows (a user's posts) WITHOUT any JSON aggregate — using `array_agg` over the child table's implicit composite type, which decodes to `Vec<..::Posts>`. When `posts=on` a LEFT JOIN + GROUP BY are added and `array_agg(p ORDER BY p.id) FILTER (WHERE p.id IS NOT NULL)` builds the composite array; when `posts=off` the join, aggregate and GROUP BY all vanish and posts comes back NULL. A single selector drives three coordinated fragments (projection, join, group-by), proving that a multi-block branch keeps a fixed result shape
--   expect: multiple
-- @end
SELECT
  u.id,
  u.name,
  #[#{posts=on} array_agg(p ORDER BY p.id) FILTER (WHERE p.id IS NOT NULL)]#[#{posts=off} NULL] AS posts
FROM public.users u
#[#{posts=on} LEFT JOIN public.posts p ON p.author_id = u.id]
WHERE u.email LIKE #{email_prefix}
#[#{posts=on} GROUP BY u.id, u.name]
ORDER BY u.id
