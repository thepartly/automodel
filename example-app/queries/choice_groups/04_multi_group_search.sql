-- @automodel
--   description: Two independent choice groups in one query. The optional `range` group selects at most one age bound (each branch carries its own per-variant field), while the required `sort` group picks a direction and shares a single `lim` argument across both of its branches
--   expect: multiple
-- @end
SELECT id, name, email, age
FROM public.users
WHERE email LIKE #{email_prefix}
  #[#{range=min?} AND age >= #{min_age?}]
  #[#{range=max?} AND age <= #{max_age?}]
#[#{sort=asc!} ORDER BY id ASC LIMIT #{lim?}]
#[#{sort=desc!} ORDER BY id DESC LIMIT #{lim?}]
