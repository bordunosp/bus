CREATE TABLE bus_jobs_peers
(
    name       VARCHAR(128) PRIMARY KEY,
    node       BINARY (16) NOT NULL,
    started_at DATETIME(6) NOT NULL,
    expires_at DATETIME(6) NOT NULL
) ENGINE=InnoDB;

CREATE TABLE bus_jobs
(
    id                    BINARY (16) PRIMARY KEY,
    hash_type_name        BIGINT       NOT NULL,

    inserted_at           DATETIME(6)      NOT NULL,
    scheduled_at          DATETIME(6)      NOT NULL,
    attempted_at          DATETIME(6)      NULL,
    completed_at          DATETIME(6)      NULL,
    cancelled_at          DATETIME(6)      NULL,
    discarded_at          DATETIME(6)      NULL,

    state                 ENUM('available', 'scheduled', 'executing',
                          'retryable', 'completed', 'cancelled',
                          'discarded')       NOT NULL,
    priority              INT          NOT NULL,
    attempt               INT          NOT NULL,
    max_attempts          INT          NOT NULL,
    execution_timeout_sec INT          NOT NULL,

    queue                 VARCHAR(128) NOT NULL,
    type_name_event       TEXT         NOT NULL,
    type_name_handler     TEXT         NOT NULL,

    payload               JSON         NOT NULL,
    meta                  JSON         NOT NULL,
    tags                  JSON         NOT NULL,
    errors                JSON         NOT NULL,
    attempted_by          JSON         NOT NULL
) ENGINE=InnoDB;

CREATE INDEX idx_bus_jobs_hash_type_name
    ON bus_jobs (hash_type_name);

CREATE INDEX idx_bus_jobs_pruning
    ON bus_jobs (state, completed_at ASC);

CREATE INDEX idx_bus_jobs_fetch_available
    ON bus_jobs (queue, state, priority DESC, scheduled_at ASC, id ASC);