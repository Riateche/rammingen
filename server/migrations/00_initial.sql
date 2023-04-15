CREATE TABLE sources (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL,
    secret VARCHAR NOT NULL
);

CREATE TABLE snapshots (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE SEQUENCE entry_update_numbers;

CREATE TABLE entries (
    id BIGSERIAL PRIMARY KEY,
    update_number BIGINT NOT NULL,
    parent_dir BIGINT REFERENCES entries(id) ON DELETE CASCADE,

    path VARCHAR NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL,
    source_id INT NOT NULL REFERENCES sources(id) ON DELETE RESTRICT,
    record_trigger INT NOT NULL,
    kind INT NOT NULL,
    exists BOOLEAN NOT NULL,
    size BIGINT,
    modified_at TIMESTAMP WITH TIME ZONE,
    content_hash bytea,
    unix_mode BIGINT
);
CREATE INDEX idx_entries_update_number ON entries (update_number);
CREATE INDEX idx_entries_path ON entries (path varchar_pattern_ops);
CREATE INDEX idx_entries_parent_dir ON entries (parent_dir);

CREATE TABLE entry_versions (
    id BIGSERIAL PRIMARY KEY,
    entry_id BIGINT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    snapshot_id INT REFERENCES snapshots(id) ON DELETE CASCADE,

    path VARCHAR NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL,
    source_id INT NOT NULL REFERENCES sources(id) ON DELETE RESTRICT,
    record_trigger INT NOT NULL,
    kind INT NOT NULL,
    exists BOOLEAN NOT NULL,
    size BIGINT,
    modified_at TIMESTAMP WITH TIME ZONE,
    content_hash bytea,
    unix_mode BIGINT
);
CREATE INDEX idx_entry_versions_entry_id ON entry_versions (entry_id);
CREATE INDEX idx_entry_versions_path ON entry_versions (path varchar_pattern_ops);


