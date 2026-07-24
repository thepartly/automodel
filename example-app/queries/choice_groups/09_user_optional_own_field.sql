-- @automodel
--   description: Conditionally project a NON-joined base-table column. When `age=on` each row carries the user's own age, when `age=off` the column comes back NULL. No join is involved — this is the single-block-per-variant projection case (each branch is one block), so it exercises the isolated-variant generator rather than the membership-based one
--   expect: multiple
-- @end
SELECT
  u.id,
  u.name,
  #[#{age=on!} u.age]#[#{age=off!} NULL] AS maybe_age
FROM public.users u
WHERE u.email LIKE #{email_prefix}
ORDER BY u.id
