-- @automodel
--   description: Conditionally return the WHOLE referrer user as a nested composite (the table row type), not just one column. When `referrer=on` a self LEFT JOIN is added and the entire referrer row is projected via `r AS referrer` (mapped to the generated `public.users` composite struct); when `referrer=off` the join is dropped and referrer comes back NULL. Row expressions are inherently nullable, so the field is `Option<..::Users>` with no false-non-null risk — and it needs no JSON aggregate
--   expect: multiple
-- @end
SELECT
  u.id,
  u.name,
  #[#{referrer=on!} r]#[#{referrer=off!} NULL] AS referrer
FROM public.users u
#[#{referrer=on!} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
ORDER BY u.id
