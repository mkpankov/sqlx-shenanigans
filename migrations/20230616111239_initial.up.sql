CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'account_id') THEN
        CREATE TYPE account_id AS (
            label text,
            audience text
        );
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'agent_id') THEN
        CREATE TYPE agent_id AS (
            account_id account_id,
            label text
        );
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL,

    PRIMARY KEY (version)
);

CREATE TABLE IF NOT EXISTS rtc (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS rtc_writer_config (
    rtc_id uuid NOT NULL,
    send_audio_updated_by agent_id,

    PRIMARY KEY (rtc_id),
    FOREIGN KEY (rtc_id) REFERENCES rtc (id) ON DELETE CASCADE
);

INSERT INTO rtc(id) VALUES
    (DEFAULT),
    (DEFAULT),
    (DEFAULT),
    (DEFAULT),
    (DEFAULT);
