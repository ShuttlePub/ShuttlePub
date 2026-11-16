-- The followers of the local actor, one row per remote actor, as projected
-- from the follower aggregates' events.
CREATE TABLE relay_followers (
    actor_id TEXT PRIMARY KEY,
    inbox    TEXT NOT NULL
);
