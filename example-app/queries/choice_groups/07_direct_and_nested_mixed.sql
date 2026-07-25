-- @automodel
--   description: Required `filter` choice group where each variant mixes a mandatory DIRECT parameter with an optional NESTED block in the same variant. `by_active` always binds `want_active` and may additionally narrow by a nested minimum age; `by_age` always binds `floor_age` and may additionally cap with a nested maximum age. This verifies direct (non-Option) and nested (Option) parameters coexist within one variant.
--   expect: multiple
-- @end
SELECT id, name, email, age, is_active
FROM public.users
WHERE name LIKE #{name_prefix}
  #[#{filter=by_active} AND is_active = #{want_active} #[AND age >= #{active_min_age?}]]
  #[#{filter=by_age}    AND age >= #{floor_age}        #[AND age <= #{ceil_age?}]]
ORDER BY id ASC
LIMIT #{lim};
