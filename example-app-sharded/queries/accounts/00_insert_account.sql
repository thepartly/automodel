-- @automodel
--    description: Insert a single account (routed by user_id)
--    expect: exactly_one
-- @end

INSERT INTO public.accounts (user_id, name, balance)
VALUES (#{user_id}, #{name}, #{balance})
RETURNING user_id, name, balance
