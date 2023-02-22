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