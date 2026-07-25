-- @automodel
-- description: Choice group whose branch binds borrowed (&[&str]) native array
--   params inside an UNNEST subquery, exercising lifetime-generic selector enums
-- types:
--   req_names: "&[&str]@native"
--   req_emails: "&[&str]@native"
-- expect: multiple
-- @end
SELECT id, name, email
FROM public.users
WHERE age > #{min_age}
  #[#{name_filter=lookup?} AND (name, email) IN (
      SELECT * FROM UNNEST(#{req_names?}::text[], #{req_emails?}::text[])
  )]
  #[#{name_filter=exact?} AND name = #{name_exact}]
  #[#{sort=n_asc?} #[AND (name, id) > (#{cur_name?}, #{cur_id?})] ORDER BY name ASC, id ASC]
  #[#{sort=n_desc?} ORDER BY name DESC, id DESC]
LIMIT #{lim};
