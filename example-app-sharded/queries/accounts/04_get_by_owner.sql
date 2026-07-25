-- @automodel
--    description: Fetch an account, sharding on an explicitly named parameter.
--                Demonstrates the per-query `shard_key` override taking
--                precedence over the global `sharding.shard_key`.
--    expect: possible_one
--    shard_key: owner_id
-- @end

SELECT user_id, name, balance
FROM public.accounts
WHERE user_id = #{owner_id}
