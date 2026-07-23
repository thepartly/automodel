-- @automodel
--   description: Keyset pagination where the first page has no cursor and later pages do. The cursor predicate is a nested optional block inside each sort variant (Option B), so each branch exposes its cursor bounds as Option fields — None yields the first page, Some yields the keyset-filtered next page.
--   expect: multiple
-- @end
SELECT id, name, email, age, is_active, created_at, updated_at
FROM public.users
WHERE name LIKE #{name_prefix}
  #[#{sort=name_asc?} #[AND (name, id) > (#{cur_name_asc_val?}, #{cur_name_asc_id?})] ORDER BY name ASC,  id ASC]
  #[#{sort=name_desc?}#[AND (name, id) < (#{cur_name_desc_val?}, #{cur_name_desc_id?})] ORDER BY name DESC, id DESC]
LIMIT #{lim};
