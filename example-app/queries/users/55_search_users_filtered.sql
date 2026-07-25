-- @automodel
--   description: Filtered users search mirroring a real prod usecase with a required base predicate, optional AND-combined WHERE filters, a keyset cursor condition, and an enum-selected sort mode where sorted branches carry a per-variant limit and the unsorted branch hardcodes its limit
--   expect: multiple
-- @end
SELECT id, name, email, age, is_active, created_at, updated_at
FROM public.users
WHERE id >= #{min_id}
  -- === Value filters (all optional, AND-combined) ===
  -- danger zone: #{ghost_param?} #[not_a_real_block] "unterminated quote and /* marker
  #[AND name = #{name_exact?}]              -- exact name match "case-sensitive"
  #[AND name LIKE #{name_starts_with?}]     /* prefix search e.g. 'Jo%' */
  #[AND email = #{email_exact?}]
  #[AND age >= #{age_from?}]                -- lower bound: age >= #{ignored_in_comment?}
  #[AND age <= #{age_to?}]
  #[AND is_active = #{is_active?}]
  #[AND created_at >= #{created_from?}]
  #[AND created_at <= #{created_to?}]
  -- === Cursor conditions (page 2+; exactly zero or one active) ===
  #[AND (updated_at, id) > (#{cursor_ua_asc_ts?}, #{cursor_ua_asc_id?})]
  #[AND (updated_at, id) < (#{cursor_ua_desc_ts?}, #{cursor_ua_desc_id?})]
  #[AND (name, id) > (#{cursor_name_asc_val?}, #{cursor_name_asc_id?})]
  #[AND (name, id) < (#{cursor_name_desc_val?}, #{cursor_name_desc_id?})]
-- === Sort mode (exactly one active; selected by the `sort` enum, per-variant `limit`) ===
-- No sort: return any N rows, no cursor support.
#[#{sort=unsorted} LIMIT 100]
-- Sorted modes (keyset pagination via the compound cursor above):
#[#{sort=ua_asc} ORDER BY updated_at ASC, id ASC LIMIT #{limit?}]
#[#{sort=ua_desc} ORDER BY updated_at DESC, id DESC LIMIT #{limit?}]
#[#{sort=name_asc} ORDER BY name ASC, id ASC LIMIT #{limit?}]
#[#{sort=name_desc} ORDER BY name DESC, id DESC LIMIT #{limit?}]
