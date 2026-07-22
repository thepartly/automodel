-- @automodel
--   description: Reproducer combining an optional `archived` choice group (mutually exclusive is_active branches), several additive optional filter blocks (UNNEST membership, name/date filters, jsonb social-links EXISTS), and an optional keyset-cursor `sort` choice group with four ORDER BY variants. No single statement can include every branch, so this exercises the group-aware type extraction (all additive blocks always included plus one variant per group)
--   expect: multiple
-- @end
SELECT id, name, email, age, is_active, created_at, updated_at
FROM public.users
WHERE id >= #{min_id}
  #[#{archived=active?} AND is_active IS TRUE]
  #[#{archived=inactive?} AND is_active IS FALSE]
  #[AND (name, email) IN (
      SELECT * FROM UNNEST(#{req_names?}::text[], #{req_emails?}::text[])
  )]
  #[AND name = #{name_exact?}]
  #[AND name LIKE #{name_starts_with?}]
  #[AND updated_at >= #{updated_from?}]
  #[AND updated_at <= #{updated_to?}]
  #[AND EXISTS (
      SELECT 1
      FROM jsonb_array_elements(profile->'social_links') AS sl
      WHERE (sl->>'platform') = ANY(#{platforms?}::text[])
  )]
  #[#{sort=ua_asc?} AND (updated_at, id) > (#{cur_ua_asc_ts}, #{cur_ua_asc_id}) ORDER BY updated_at ASC, id ASC]
  #[#{sort=ua_desc?} AND (updated_at, id) < (#{cur_ua_desc_ts}, #{cur_ua_desc_id}) ORDER BY updated_at DESC, id DESC]
  #[#{sort=name_asc?} AND (name, id) > (#{cur_name_asc_val}, #{cur_name_asc_id}) ORDER BY name ASC, id ASC]
  #[#{sort=name_desc?} AND (name, id) < (#{cur_name_desc_val}, #{cur_name_desc_id}) ORDER BY name DESC, id DESC]
LIMIT #{lim};
