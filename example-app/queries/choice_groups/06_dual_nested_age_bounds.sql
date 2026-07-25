-- @automodel
--   description: Required `sort` choice group where each variant carries TWO independent nested optional blocks (an optional lower and upper age bound). This exercises multiple nested optional blocks inside a single required variant — any combination of the two bounds may be omitted, and each variant renders its own ORDER BY.
--   expect: multiple
-- @end
SELECT id, name, email, age
FROM public.users
WHERE name LIKE #{name_prefix}
  #[#{sort=asc}  #[AND age >= #{asc_min_age?}]  #[AND age <= #{asc_max_age?}]  ORDER BY age ASC,  id ASC]
  #[#{sort=desc} #[AND age >= #{desc_min_age?}] #[AND age <= #{desc_max_age?}] ORDER BY age DESC, id DESC]
LIMIT #{lim};
