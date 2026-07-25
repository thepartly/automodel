-- @automodel
--   description: Mixed query — additive AND-combined age filters coexist with an enum-selected sort mode. The `unsorted` branch binds no parameters (it hardcodes its LIMIT, generating a unit variant), while the sorted branches share a per-variant `limit` field
--   expect: multiple
-- @end
SELECT id, name, email, age
FROM public.users
WHERE email LIKE #{email_prefix}
  #[AND age >= #{min_age?}]
  #[AND age <= #{max_age?}]
#[#{sort=unsorted} LIMIT 100]
#[#{sort=age_asc} ORDER BY age ASC, id ASC LIMIT #{limit?}]
#[#{sort=age_desc} ORDER BY age DESC, id DESC LIMIT #{limit?}]
