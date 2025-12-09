BEGIN;

ALTER TABLE entry_versions
ADD is_symlink BOOLEAN DEFAULT NULL;

ALTER TABLE entries
ADD is_symlink BOOLEAN DEFAULT NULL;

CREATE OR REPLACE FUNCTION on_entry_update()
   RETURNS TRIGGER
   LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO entry_versions (
        entry_id, update_number, snapshot_id, path, recorded_at, source_id,
        record_trigger, kind, original_size, encrypted_size, modified_at, content_hash, unix_mode, is_symlink
    ) VALUES (
        NEW.id, NEW.update_number, NULL, NEW.path, NEW.recorded_at, NEW.source_id,
        NEW.record_trigger, NEW.kind, NEW.original_size, NEW.encrypted_size,
        NEW.modified_at, NEW.content_hash, NEW.unix_mode, NEW.is_symlink
    );
    RETURN NULL;
END;
$$;

UPDATE entries SET is_symlink = false WHERE kind = 1;
UPDATE entry_versions SET is_symlink = false WHERE kind = 1;

COMMIT;
