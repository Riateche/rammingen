CREATE TABLE sources (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL UNIQUE,
    access_token VARCHAR NOT NULL UNIQUE
);

CREATE TABLE snapshots (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL
);
CREATE INDEX idx_snapshots_timestamp ON snapshots (timestamp);

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
    original_size bytea,
    encrypted_size BIGINT,
    modified_at TIMESTAMP WITH TIME ZONE,
    content_hash bytea,
    unix_mode BIGINT
);
CREATE INDEX idx_entries_update_number ON entries (update_number);
CREATE INDEX idx_entries_path ON entries (path varchar_pattern_ops);
CREATE INDEX idx_entries_parent_dir ON entries (parent_dir);
CREATE INDEX idx_entries_recorded_at ON entries (recorded_at);
CREATE INDEX idx_entries_content_hash ON entries (content_hash);

CREATE TABLE entry_versions (
    id BIGSERIAL PRIMARY KEY,
    entry_id BIGINT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    update_number BIGINT NOT NULL,
    snapshot_id INT REFERENCES snapshots(id) ON DELETE CASCADE,

    path VARCHAR NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL,
    source_id INT NOT NULL REFERENCES sources(id) ON DELETE RESTRICT,
    record_trigger INT NOT NULL,
    kind INT NOT NULL,
    original_size bytea,
    encrypted_size BIGINT,
    modified_at TIMESTAMP WITH TIME ZONE,
    content_hash bytea,
    unix_mode BIGINT
);
CREATE INDEX idx_entry_versions_entry_id ON entry_versions (entry_id);
CREATE INDEX idx_entry_versions_update_number ON entry_versions (update_number);
CREATE INDEX idx_entry_versions_snapshot_id ON entry_versions (snapshot_id);
CREATE INDEX idx_entry_versions_path ON entry_versions (path varchar_pattern_ops);
CREATE INDEX idx_entry_versions_recorded_at ON entry_versions (recorded_at);
CREATE INDEX idx_entry_versions_content_hash ON entry_versions (content_hash);

CREATE FUNCTION on_entry_update()
   RETURNS TRIGGER
   LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO entry_versions (
        entry_id, update_number, snapshot_id, path, recorded_at, source_id,
        record_trigger, kind, original_size, encrypted_size, modified_at, content_hash, unix_mode
    ) VALUES (
        NEW.id, NEW.update_number, NULL, NEW.path, NEW.recorded_at, NEW.source_id,
        NEW.record_trigger, NEW.kind, NEW.original_size, NEW.encrypted_size,
        NEW.modified_at, NEW.content_hash, NEW.unix_mode
    );
    RETURN NULL;
END;
$$;

CREATE TRIGGER trigger_after_entries_insert_or_update
    AFTER INSERT OR UPDATE ON entries
    FOR EACH ROW
    EXECUTE FUNCTION on_entry_update();
