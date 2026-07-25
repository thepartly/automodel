-- @automodel
--    description: Update an account balance (routed by user_id)
--    expect: exactly_one
-- @end

UPDATE public.accounts
SET balance = #{balance}
WHERE user_id = #{user_id}
RETURNING user_id, name, balance
