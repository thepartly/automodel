-- @automodel
--   description: TWO independent selectors in ONE query. `referrer` toggles a whole-row composite (self LEFT JOIN + `r AS referrer` mapped to `Option<..::Users>`); `posts` toggles a child collection built from a correlated `array_agg` subquery mapped to `Option<Vec<..::Posts>>`. The selectors are orthogonal — all four On/Off combinations yield a valid fixed-shape row — and the posts subquery deliberately avoids GROUP BY so it composes freely with the join. Note both off-branches are the identical literal `NULL`; the generator addresses each block positionally so the two bodies do not collide. No JSON aggregate is used
--   expect: multiple
-- @end
SELECT
  u.id,
  u.name,
  #[#{referrer=on!} r]#[#{referrer=off!} NULL] AS referrer,
  #[#{posts=on!} (SELECT array_agg(p ORDER BY p.id) FROM public.posts p WHERE p.author_id = u.id)]#[#{posts=off!} NULL] AS posts
FROM public.users u
#[#{referrer=on!} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
ORDER BY u.id
