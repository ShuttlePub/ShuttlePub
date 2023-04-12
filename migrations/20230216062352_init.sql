CREATE TABLE accounts (
  id BIGSERIAL NOT NULL PRIMARY KEY UNIQUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  name VARCHAR(50) NOT NULL UNIQUE,
  bot BOOLEAN NOT NULL
);

CREATE TABLE profiles (
  id UUID NOT NULL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  account BIGSERIAL NOT NULL,
  display_name VARCHAR(64),
  summary TEXT,
  icon VARCHAR(256),
  banner VARCHAR(256),

  FOREIGN KEY (account) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE TABLE metadata (
  id UUID NOT NULL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  account BIGSERIAL,
  label VARCHAR(16) NOT NULL,
  content VARCHAR(256),

  FOREIGN KEY (account) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE TABLE follows (
  id UUID NOT NULL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  source BIGSERIAL NOT NULL,
  destination_local BIGSERIAL,
  destination_remote VARCHAR(512),

  FOREIGN KEY (source) REFERENCES accounts(id) ON DELETE CASCADE,
  FOREIGN KEY (destination_local) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE TABLE confidentials (
  id UUID NOT NULL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  account BIGSERIAL NOT NULL,
  address VARCHAR(128) NOT NULL,
  pass VARCHAR(256) NOT NULL,

  FOREIGN KEY (account) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE SEQUENCE auto_increment
  AS INTEGER
  START WITH 1
  INCREMENT BY 1
  NO MINVALUE
  NO MAXVALUE
  CACHE 1;

ALTER SEQUENCE auto_increment OWNED BY profiles.id;
ALTER SEQUENCE auto_increment OWNED BY metadata.id;
ALTER SEQUENCE auto_increment OWNED BY confidentials.id;

CREATE TABLE notes (
  id UUID NOT NULL PRIMARY KEY,
  account BIGSERIAL NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  hashtag UUID[] NOT NULL,
  media   UUID[] NOT NULL,
  content TEXT   NOT NULL,
  cw      VARCHAR(512),

  FOREIGN KEY (account) REFERENCES accounts(id) ON DELETE CASCADE,
  FOREIGN KEY (EACH ELEMENT OF mention) REFERENCES accounts(id)   ON DELETE CASCADE,
  FOREIGN KEY (EACH ELEMENT OF media)   REFERENCES note_media(id) ON DELETE CASCADE,
  FOREIGN KEY (EACH ELEMENT OF hashtag) REFERENCES hashtags(id)   ON DELETE CASCADE
);

CREATE TABLE note_media (
  id          UUID    NOT NULL PRIMARY KEY,
  sensitive   BOOLEAN NOT NULL,
  description VARCHAR(512),
  content     VARCHAR(512),

  license_url  VARCHAR(512),
  license_spdx VARCHAR(128)
);

CREATE TABLE note_reaction (
  id   UUID NOT NULL PRIMARY KEY,
  note UUID NOT NULL,
  account BIGSERIAL NOT NULL,

  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  FOREIGN KEY (note)    REFERENCES note(id)    ON DELETE CASCADE,
  FOREIGN KEY (account) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE note_reply (
  id UUID NOT NULL PRIMARY KEY,

  origin_local UUID,
  origin_remote VARCHAR(512),

  target_local UUID,
  target_remote VARCHAR(512),

  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  FOREIGN KEY (origin_local) REFERENCES notes(id) ON DELETE CASCADE,
  FOREIGN KEY (target_local) REFERENCES notes(id) ON DELETE CASCADE
);

CREATE TABLE note_mention (
  id UUID NOT NULL PRIMARY KEY,

  origin_local UUID,

  target_local UUID,
  target_remote VARCHAR(512),

  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  FOREIGN KEY (origin_local) REFERENCES notes(id) ON DELETE CASCADE,
  FOREIGN KEY (target_local) REFERENCES notes(id) ON DELETE CASCADE
);

CREATE TABLE note_turbo_quote (
  id UUID NOT NULL PRIMARY KEY,
  
  origin_local UUID,
  origin_remote VARCHAR(512),

  target_local UUID,
  target_remote VARCHAR(512),

  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  implicit boolean NOT NULL,

  FOREIGN KEY (origin_local) REFERENCES notes(id) ON DELETE CASCADE,
  FOREIGN KEY (target_local) REFERENCES notes(id) ON DELETE CASCADE
);

CREATE TABLE note_turbo (
  id UUID NOT NULL PRIMARY KEY,
  
  origin_account BIGSERIAL,
  origin_remote VARCHAR(512),

  target_local UUID,
  target_remote VARCHAR(512),

  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  implicit boolean NOT NULL,

  FOREIGN KEY (origin_local) REFERENCES account(id) ON DELETE CASCADE,
  FOREIGN KEY (target_local) REFERENCES notes(id) ON DELETE CASCADE
);

CREATE TABLE reaction_asset (
  id    UUID NOT NULL PRIMARY KEY,
  alias VARCHAR(128) NOT NULL UNIQUE,
  asset VARCHAR(512) NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),

  license_url  VARCHAR(512),
  license_spdx VARCHAR(128)
);

CREATE TABLE hashtags (
  id   UUID         NOT NULL PRIMARY KEY,
  name VARCHAR(128) NOT NULL UNIQUE,

  created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
);

