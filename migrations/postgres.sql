CREATE TYPE bus_job_state AS ENUM (
  'available',
  'scheduled',
  'executing',
  'retryable',
  'completed',
  'cancelled',
  'discarded'
);

CREATE TABLE bus_jobs_peers
(
    name       VARCHAR(128) PRIMARY KEY,
    node       UUID        NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE bus_jobs
(
    id                    UUID PRIMARY KEY,
    hash_type_name        BIGINT        NOT NULL,

    inserted_at           TIMESTAMPTZ   NOT NULL,
    scheduled_at          TIMESTAMPTZ   NOT NULL,
    attempted_at          TIMESTAMPTZ,
    completed_at          TIMESTAMPTZ,
    cancelled_at          TIMESTAMPTZ,
    discarded_at          TIMESTAMPTZ,

    state                 bus_job_state NOT NULL,
    priority              INTEGER       NOT NULL,
    attempt               INTEGER       NOT NULL,
    max_attempts          INTEGER       NOT NULL,
    execution_timeout_sec INTEGER       NOT NULL,

    queue                 VARCHAR(128)  NOT NULL,
    type_name_event       TEXT          NOT NULL,
    type_name_handler     TEXT          NOT NULL,

    payload               JSONB         NOT NULL,
    meta                  JSONB         NOT NULL,
    tags                  JSONB         NOT NULL,
    errors                JSONB         NOT NULL,
    attempted_by          JSONB         NOT NULL
);

CREATE INDEX idx_bus_jobs_hash_type_name
    ON bus_jobs (hash_type_name);

CREATE INDEX idx_bus_jobs_pruning
    ON bus_jobs (state, completed_at ASC);

CREATE INDEX idx_bus_jobs_fetch_available
    ON bus_jobs (queue, state, priority DESC, scheduled_at ASC, id ASC);