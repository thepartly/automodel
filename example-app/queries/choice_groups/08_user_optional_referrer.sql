-- @automodel
--   description: Fetch users by email prefix; when `referrer=on` each row also carries the referrer's age via a joined self-reference (users.referrer_id), and when `referrer=off` the LEFT JOIN is skipped entirely and referrer_age comes back NULL. Demonstrates a single selector driving two coordinated fragments (projection + join) that switch together while keeping a fixed result shape
--   expect: multiple
-- @end
SELECT
  u.id,
  u.name,
  #[#{referrer=on} r.age]#[#{referrer=off} NULL] AS referrer_age
FROM public.users u
#[#{referrer=on} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
ORDER BY u.id
