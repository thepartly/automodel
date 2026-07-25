-- @automodel
--    description: Bulk insert accounts that all belong to the same shard.
--                The generated code verifies every row resolves to the same
--                shard key before routing, returning ShardError::InconsistentBatch
--                otherwise.
--    expect: multiple
--    multiunzip: true
-- @end

INSERT INTO public.accounts (user_id, name, balance)
SELECT * FROM UNNEST(#{user_id}::uuid[], #{name}::text[], #{balance}::bigint[])
RETURNING user_id, name, balance
