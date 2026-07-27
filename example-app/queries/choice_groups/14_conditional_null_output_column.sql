-- @automodel
--   description: Output column conditionally replaced by a NULL literal via a choice group
--   expect: multiple
--   return_type: ConditionalNullOutputColumnRow
-- @end
SELECT
    id,
    #[#{name_incl=on} name]#[#{name_incl=off} NULL::text] AS name
FROM public.users
WHERE age >= #{min_age}
ORDER BY id ASC
