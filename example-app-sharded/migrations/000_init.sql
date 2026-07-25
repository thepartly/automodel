-- Schema for the sharded example app. In a real deployment this schema would be
-- applied identically to every physical shard database. Here a single database
-- backs all logical shards, which is sufficient to exercise the generated
-- routing code end-to-end.

CREATE TABLE IF NOT EXISTS accounts (
    user_id uuid PRIMARY KEY,
    name    text   NOT NULL,
    balance bigint NOT NULL DEFAULT 0
);
