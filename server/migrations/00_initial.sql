CREATE TABLE sources (
    id SERIAL PRIMARY KEY,
    name VARCHAR,
    secret VARCHAR
);

CREATE TABLE snapshots (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE TABLE entries (
    id BIGSERIAL PRIMARY KEY,
    path VARCHAR NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL,
    source_id INT NOT NULL REFERENCES sources(id) ON DELETE RESTRICT,
    record_trigger INT NOT NULL,
    parent_dir INT REFERENCES entries(id) ON DELETE CASCADE,
    kind INT NOT NULL,
    exists BOOLEAN NOT NULL,
    size BIGINT,
    modified_at TIMESTAMP WITH TIME ZONE,
    content_hash bytea,
    unix_mode BIGINT
);
CREATE INDEX idx_entries_path ON entries (path varchar_pattern_ops);
CREATE INDEX idx_entries_parent_dir ON entries (parent_dir);

CREATE TABLE entry_versions (
    entry_id BIGINT REFERENCES entries(id),
    path VARCHAR NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL,
    source_id INT NOT NULL REFERENCES sources(id) ON DELETE RESTRICT,
    record_trigger INT NOT NULL,
    parent_dir INT REFERENCES entries(id) ON DELETE CASCADE,
    snapshot_id INT REFERENCES snapshots(id) ON DELETE CASCADE,
    kind INT NOT NULL,
    exists BOOLEAN NOT NULL,
    size BIGINT,
    modified_at TIMESTAMP WITH TIME ZONE,
    content_hash bytea,
    unix_mode BIGINT
);
CREATE INDEX idx_entry_versions_entry_id ON entry_versions (entry_id);
CREATE INDEX idx_entry_versions_path ON entry_versions (path varchar_pattern_ops);
CREATE INDEX idx_entry_versions_parent_dir ON entry_versions (parent_dir);
