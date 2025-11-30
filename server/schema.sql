--
-- PostgreSQL database dump
--

\restrict 4fMf7s7LkeUnpOYwqrIrzhy0qDLJ37kw0pHUNidUcDOp45sOOa3HUK3oJiDKPsL

-- Dumped from database version 18.0
-- Dumped by pg_dump version 18.0

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: on_entry_update(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.on_entry_update() RETURNS trigger
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


ALTER FUNCTION public.on_entry_update() OWNER TO postgres;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


ALTER TABLE public._sqlx_migrations OWNER TO postgres;

--
-- Name: entries; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.entries (
    id bigint NOT NULL,
    update_number bigint NOT NULL,
    parent_dir bigint,
    path character varying NOT NULL,
    recorded_at timestamp with time zone NOT NULL,
    source_id integer NOT NULL,
    record_trigger integer NOT NULL,
    kind integer NOT NULL,
    original_size bytea,
    encrypted_size bigint,
    modified_at timestamp with time zone,
    content_hash bytea,
    unix_mode bigint,
    is_symlink boolean
);


ALTER TABLE public.entries OWNER TO postgres;

--
-- Name: entries_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.entries_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.entries_id_seq OWNER TO postgres;

--
-- Name: entries_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.entries_id_seq OWNED BY public.entries.id;


--
-- Name: entry_update_numbers; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.entry_update_numbers
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.entry_update_numbers OWNER TO postgres;

--
-- Name: entry_versions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.entry_versions (
    id bigint NOT NULL,
    entry_id bigint NOT NULL,
    update_number bigint NOT NULL,
    snapshot_id integer,
    path character varying NOT NULL,
    recorded_at timestamp with time zone NOT NULL,
    source_id integer NOT NULL,
    record_trigger integer NOT NULL,
    kind integer NOT NULL,
    original_size bytea,
    encrypted_size bigint,
    modified_at timestamp with time zone,
    content_hash bytea,
    unix_mode bigint,
    is_symlink boolean
);


ALTER TABLE public.entry_versions OWNER TO postgres;

--
-- Name: entry_versions_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.entry_versions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.entry_versions_id_seq OWNER TO postgres;

--
-- Name: entry_versions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.entry_versions_id_seq OWNED BY public.entry_versions.id;


--
-- Name: server_id; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.server_id (
    server_id character varying NOT NULL
);


ALTER TABLE public.server_id OWNER TO postgres;

--
-- Name: snapshots; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.snapshots (
    id integer NOT NULL,
    "timestamp" timestamp with time zone NOT NULL
);


ALTER TABLE public.snapshots OWNER TO postgres;

--
-- Name: snapshots_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.snapshots_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.snapshots_id_seq OWNER TO postgres;

--
-- Name: snapshots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.snapshots_id_seq OWNED BY public.snapshots.id;


--
-- Name: sources; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.sources (
    id integer NOT NULL,
    name character varying NOT NULL,
    access_token character varying NOT NULL
);


ALTER TABLE public.sources OWNER TO postgres;

--
-- Name: sources_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.sources_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.sources_id_seq OWNER TO postgres;

--
-- Name: sources_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.sources_id_seq OWNED BY public.sources.id;


--
-- Name: entries id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entries ALTER COLUMN id SET DEFAULT nextval('public.entries_id_seq'::regclass);


--
-- Name: entry_versions id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entry_versions ALTER COLUMN id SET DEFAULT nextval('public.entry_versions_id_seq'::regclass);


--
-- Name: snapshots id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.snapshots ALTER COLUMN id SET DEFAULT nextval('public.snapshots_id_seq'::regclass);


--
-- Name: sources id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sources ALTER COLUMN id SET DEFAULT nextval('public.sources_id_seq'::regclass);


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: entries entries_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entries
    ADD CONSTRAINT entries_pkey PRIMARY KEY (id);


--
-- Name: entry_versions entry_versions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entry_versions
    ADD CONSTRAINT entry_versions_pkey PRIMARY KEY (id);


--
-- Name: snapshots snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.snapshots
    ADD CONSTRAINT snapshots_pkey PRIMARY KEY (id);


--
-- Name: sources sources_access_token_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_access_token_key UNIQUE (access_token);


--
-- Name: sources sources_name_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_name_key UNIQUE (name);


--
-- Name: sources sources_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_pkey PRIMARY KEY (id);


--
-- Name: idx_entries_content_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entries_content_hash ON public.entries USING btree (content_hash);


--
-- Name: idx_entries_parent_dir; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entries_parent_dir ON public.entries USING btree (parent_dir);


--
-- Name: idx_entries_path; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entries_path ON public.entries USING btree (path varchar_pattern_ops);


--
-- Name: idx_entries_recorded_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entries_recorded_at ON public.entries USING btree (recorded_at);


--
-- Name: idx_entries_update_number; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entries_update_number ON public.entries USING btree (update_number);


--
-- Name: idx_entry_versions_content_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entry_versions_content_hash ON public.entry_versions USING btree (content_hash);


--
-- Name: idx_entry_versions_entry_id; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entry_versions_entry_id ON public.entry_versions USING btree (entry_id);


--
-- Name: idx_entry_versions_path; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entry_versions_path ON public.entry_versions USING btree (path varchar_pattern_ops);


--
-- Name: idx_entry_versions_recorded_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entry_versions_recorded_at ON public.entry_versions USING btree (recorded_at);


--
-- Name: idx_entry_versions_snapshot_id; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entry_versions_snapshot_id ON public.entry_versions USING btree (snapshot_id);


--
-- Name: idx_entry_versions_update_number; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_entry_versions_update_number ON public.entry_versions USING btree (update_number);


--
-- Name: idx_snapshots_timestamp; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_snapshots_timestamp ON public.snapshots USING btree ("timestamp");


--
-- Name: entries trigger_after_entries_insert_or_update; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER trigger_after_entries_insert_or_update AFTER INSERT OR UPDATE ON public.entries FOR EACH ROW EXECUTE FUNCTION public.on_entry_update();


--
-- Name: entries entries_parent_dir_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entries
    ADD CONSTRAINT entries_parent_dir_fkey FOREIGN KEY (parent_dir) REFERENCES public.entries(id) ON DELETE CASCADE;


--
-- Name: entries entries_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entries
    ADD CONSTRAINT entries_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE RESTRICT;


--
-- Name: entry_versions entry_versions_entry_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entry_versions
    ADD CONSTRAINT entry_versions_entry_id_fkey FOREIGN KEY (entry_id) REFERENCES public.entries(id) ON DELETE CASCADE;


--
-- Name: entry_versions entry_versions_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entry_versions
    ADD CONSTRAINT entry_versions_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES public.snapshots(id) ON DELETE CASCADE;


--
-- Name: entry_versions entry_versions_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.entry_versions
    ADD CONSTRAINT entry_versions_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE RESTRICT;


--
-- PostgreSQL database dump complete
--

\unrestrict 4fMf7s7LkeUnpOYwqrIrzhy0qDLJ37kw0pHUNidUcDOp45sOOa3HUK3oJiDKPsL

