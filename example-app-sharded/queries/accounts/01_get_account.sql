-- @automodel
--    description: Fetch an account by id (routed by user_id)
--    expect: possible_one
-- @end

SELECT user_id, name, balance
FROM public.accounts
WHERE user_id = #{user_id}
