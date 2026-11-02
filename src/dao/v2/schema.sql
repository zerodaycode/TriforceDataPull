-- Things Canyon can't express (unique indexes, checks, lookup indexes). Run at every start, idempotent.
-- Never add foreign keys here: Canyon drops FKs that aren't in the entities.
-- Changing an index: give it a new name and DROP INDEX IF EXISTS the old one (IF NOT EXISTS keeps the old definition).
-- One statement ends at a line ending in ';' (comment lines never end one): keep DO blocks on one line.
CREATE UNIQUE INDEX IF NOT EXISTS v2_league_ext_id ON v2_league (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_league_i18n_key ON v2_league_i18n (league_id, locale);
CREATE UNIQUE INDEX IF NOT EXISTS v2_season_ext_id ON v2_season (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_season_i18n_key ON v2_season_i18n (season_id, locale);
CREATE UNIQUE INDEX IF NOT EXISTS v2_split_ext_id ON v2_split (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_tournament_ext_id ON v2_tournament (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_tournament_i18n_key ON v2_tournament_i18n (tournament_id, locale);
CREATE UNIQUE INDEX IF NOT EXISTS v2_stage_ext_id ON v2_stage (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_stage_i18n_key ON v2_stage_i18n (stage_id, locale);
CREATE UNIQUE INDEX IF NOT EXISTS v2_team_ext_id ON v2_team (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_player_ext_id ON v2_player (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_team_player_key ON v2_team_player (team_id, player_id);
CREATE INDEX IF NOT EXISTS v2_team_player_player ON v2_team_player (player_id);
CREATE INDEX IF NOT EXISTS v2_tournament_league ON v2_tournament (league_id);
ALTER TABLE v2_league_i18n DROP CONSTRAINT IF EXISTS v2_league_i18n_locale, ADD CONSTRAINT v2_league_i18n_locale CHECK (locale ~ '^[a-z]{2}-[A-Z]{2}$');
ALTER TABLE v2_season_i18n DROP CONSTRAINT IF EXISTS v2_season_i18n_locale, ADD CONSTRAINT v2_season_i18n_locale CHECK (locale ~ '^[a-z]{2}-[A-Z]{2}$');
ALTER TABLE v2_tournament_i18n DROP CONSTRAINT IF EXISTS v2_tournament_i18n_locale, ADD CONSTRAINT v2_tournament_i18n_locale CHECK (locale ~ '^[a-z]{2}-[A-Z]{2}$');
ALTER TABLE v2_stage_i18n DROP CONSTRAINT IF EXISTS v2_stage_i18n_locale, ADD CONSTRAINT v2_stage_i18n_locale CHECK (locale ~ '^[a-z]{2}-[A-Z]{2}$');
CREATE UNIQUE INDEX IF NOT EXISTS v2_event_ext_id ON v2_event (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_event_i18n_key ON v2_event_i18n (event_id, locale);
CREATE UNIQUE INDEX IF NOT EXISTS v2_event_team_key ON v2_event_team (event_id, slot);
CREATE UNIQUE INDEX IF NOT EXISTS v2_game_ext_id ON v2_game (ext_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_game_key ON v2_game (event_id, number);
CREATE UNIQUE INDEX IF NOT EXISTS v2_stream_key ON v2_stream (event_id, provider, parameter);
CREATE UNIQUE INDEX IF NOT EXISTS v2_job_run_job ON v2_job_run (job);
CREATE INDEX IF NOT EXISTS v2_event_start_time ON v2_event (start_time);
CREATE INDEX IF NOT EXISTS v2_event_state ON v2_event (state);
CREATE INDEX IF NOT EXISTS v2_event_team_team ON v2_event_team (team_id);
CREATE INDEX IF NOT EXISTS v2_event_league ON v2_event (league_id);
CREATE INDEX IF NOT EXISTS v2_event_tournament ON v2_event (tournament_id);
ALTER TABLE v2_event DROP CONSTRAINT IF EXISTS v2_event_kind, ADD CONSTRAINT v2_event_kind CHECK (kind IN ('match', 'show'));
ALTER TABLE v2_event DROP CONSTRAINT IF EXISTS v2_event_state_check, ADD CONSTRAINT v2_event_state_check CHECK (state IN ('unstarted', 'inProgress', 'completed'));
ALTER TABLE v2_event DROP CONSTRAINT IF EXISTS v2_event_strategy, ADD CONSTRAINT v2_event_strategy CHECK (strategy_type IN ('bestOf', 'playAll'));
ALTER TABLE v2_event DROP CONSTRAINT IF EXISTS v2_event_match_type, ADD CONSTRAINT v2_event_match_type CHECK (match_type IN ('normal', 'tiebreaker'));
ALTER TABLE v2_event_i18n DROP CONSTRAINT IF EXISTS v2_event_i18n_locale, ADD CONSTRAINT v2_event_i18n_locale CHECK (locale ~ '^[a-z]{2}-[A-Z]{2}$');
ALTER TABLE v2_event_team DROP CONSTRAINT IF EXISTS v2_event_team_slot, ADD CONSTRAINT v2_event_team_slot CHECK (slot IN (1, 2));
ALTER TABLE v2_event_team DROP CONSTRAINT IF EXISTS v2_event_team_outcome, ADD CONSTRAINT v2_event_team_outcome CHECK (outcome IN ('win', 'loss', 'tie'));
ALTER TABLE v2_game DROP CONSTRAINT IF EXISTS v2_game_state_check, ADD CONSTRAINT v2_game_state_check CHECK (state IN ('unstarted', 'inProgress', 'completed', 'unneeded'));
ALTER TABLE v2_game DROP CONSTRAINT IF EXISTS v2_game_number, ADD CONSTRAINT v2_game_number CHECK (number >= 1);
-- Same ranks as rows::state_rank in Rust: a state never goes down (unneeded counts as done)
CREATE OR REPLACE FUNCTION v2_state_rank(s text) RETURNS int IMMUTABLE LANGUAGE sql AS $$ SELECT CASE s WHEN 'inProgress' THEN 1 WHEN 'completed' THEN 2 WHEN 'unneeded' THEN 2 ELSE 0 END $$;
CREATE UNIQUE INDEX IF NOT EXISTS v2_game_team_key ON v2_game_team (game_id, team_id);
CREATE UNIQUE INDEX IF NOT EXISTS v2_game_player_key ON v2_game_player (game_id, participant_id);
CREATE INDEX IF NOT EXISTS v2_game_team_team ON v2_game_team (team_id);
CREATE INDEX IF NOT EXISTS v2_game_player_player ON v2_game_player (player_id);
CREATE INDEX IF NOT EXISTS v2_game_player_team ON v2_game_player (team_id);
CREATE INDEX IF NOT EXISTS v2_game_queue ON v2_game (event_id) WHERE details IS NULL AND state = 'completed';
ALTER TABLE v2_game DROP CONSTRAINT IF EXISTS v2_game_details, ADD CONSTRAINT v2_game_details CHECK (details IN ('done', 'no_feed'));
ALTER TABLE v2_game_team DROP CONSTRAINT IF EXISTS v2_game_team_side, ADD CONSTRAINT v2_game_team_side CHECK (side IN ('blue', 'red'));
ALTER TABLE v2_game_team DROP CONSTRAINT IF EXISTS v2_game_team_source, ADD CONSTRAINT v2_game_team_source CHECK (side_source IN ('api', 'feed'));
ALTER TABLE v2_game_team DROP CONSTRAINT IF EXISTS v2_game_team_result, ADD CONSTRAINT v2_game_team_result CHECK (result IN ('win', 'loss'));
ALTER TABLE v2_game_player DROP CONSTRAINT IF EXISTS v2_game_player_participant, ADD CONSTRAINT v2_game_player_participant CHECK (participant_id BETWEEN 1 AND 10);
-- Game frames (not Canyon entities: Canyon has no bytea or arrays, and never touches a table it doesn't know)
ALTER TABLE v2_game DROP CONSTRAINT IF EXISTS v2_game_frames_state, ADD CONSTRAINT v2_game_frames_state CHECK (frames IN ('live', 'done', 'no_feed'));
-- same predicate as the frames queue (IS DISTINCT FROM can't be matched by the planner)
DROP INDEX IF EXISTS v2_game_frames_queue;
CREATE INDEX IF NOT EXISTS v2_game_frames_todo ON v2_game (event_id) WHERE state = 'completed' AND (frames IS NULL OR frames = 'live');
CREATE INDEX IF NOT EXISTS v2_game_in_progress ON v2_game (id) WHERE state = 'inProgress';
-- Raw archive: every frame of an endpoint, zstd JSON ({"gameMetadata", "frames"} for format 1)
CREATE TABLE IF NOT EXISTS v2_game_frames (game_id int NOT NULL REFERENCES v2_game (id), endpoint text NOT NULL, format smallint NOT NULL, frame_count int NOT NULL, first_frame timestamp NOT NULL, last_frame timestamp NOT NULL, raw_bytes int NOT NULL, fetched_at timestamp NOT NULL, data bytea NOT NULL, PRIMARY KEY (game_id, endpoint));
-- already compressed: no TOAST compression on top
ALTER TABLE v2_game_frames ALTER COLUMN data SET STORAGE EXTERNAL;
ALTER TABLE v2_game_frames DROP CONSTRAINT IF EXISTS v2_game_frames_endpoint, ADD CONSTRAINT v2_game_frames_endpoint CHECK (endpoint IN ('window', 'details'));
-- Series: one point every step_s of game time, every array `points` long
CREATE TABLE IF NOT EXISTS v2_game_timeline (game_id int PRIMARY KEY REFERENCES v2_game (id), source text NOT NULL, step_s smallint NOT NULL, first_frame timestamp NOT NULL, duration_s int NOT NULL, points int NOT NULL, wall_time timestamp[] NOT NULL, live_state jsonb, built_at timestamp NOT NULL);
ALTER TABLE v2_game_timeline DROP CONSTRAINT IF EXISTS v2_game_timeline_source, ADD CONSTRAINT v2_game_timeline_source CHECK (source IN ('live', 'archive'));
ALTER TABLE v2_game_timeline DROP CONSTRAINT IF EXISTS v2_game_timeline_step, ADD CONSTRAINT v2_game_timeline_step CHECK (step_s > 0);
CREATE TABLE IF NOT EXISTS v2_game_team_series (game_id int NOT NULL REFERENCES v2_game (id), team_id int NOT NULL REFERENCES v2_team (id), side text NOT NULL, gold int[] NOT NULL, kills int[] NOT NULL, towers int[] NOT NULL, inhibitors int[] NOT NULL, barons int[] NOT NULL, dragons int[] NOT NULL, PRIMARY KEY (game_id, team_id));
ALTER TABLE v2_game_team_series DROP CONSTRAINT IF EXISTS v2_game_team_series_side, ADD CONSTRAINT v2_game_team_series_side CHECK (side IN ('blue', 'red'));
CREATE TABLE IF NOT EXISTS v2_game_player_series (game_id int NOT NULL REFERENCES v2_game (id), participant_id smallint NOT NULL, gold int[] NOT NULL, level int[] NOT NULL, kills int[] NOT NULL, deaths int[] NOT NULL, assists int[] NOT NULL, cs int[] NOT NULL, hp int[] NOT NULL, max_hp int[] NOT NULL, gold_earned int[] NOT NULL, kill_participation real[] NOT NULL, damage_share real[] NOT NULL, wards_placed int[] NOT NULL, wards_destroyed int[] NOT NULL, attack_damage int[] NOT NULL, ability_power int[] NOT NULL, crit_chance real[] NOT NULL, attack_speed int[] NOT NULL, life_steal int[] NOT NULL, armor int[] NOT NULL, magic_resist int[] NOT NULL, tenacity real[] NOT NULL, items int[] NOT NULL, rune_style int, rune_sub_style int, runes int[], skill_order text[] NOT NULL, skill_time int[] NOT NULL, PRIMARY KEY (game_id, participant_id));
ALTER TABLE v2_game_player_series DROP CONSTRAINT IF EXISTS v2_game_player_series_participant, ADD CONSTRAINT v2_game_player_series_participant CHECK (participant_id BETWEEN 1 AND 10);
CREATE TABLE IF NOT EXISTS v2_game_marker (game_id int NOT NULL REFERENCES v2_game (id), t int NOT NULL, kind text NOT NULL, team_id int NOT NULL REFERENCES v2_team (id), detail text);
ALTER TABLE v2_game_marker DROP CONSTRAINT IF EXISTS v2_game_marker_kind, ADD CONSTRAINT v2_game_marker_kind CHECK (kind IN ('dragon', 'baron', 'tower', 'inhibitor'));
CREATE INDEX IF NOT EXISTS v2_game_marker_game ON v2_game_marker (game_id, t);
CREATE INDEX IF NOT EXISTS v2_game_team_series_team ON v2_game_team_series (team_id);
-- jsonb arrays (how the series are sent: Canyon binds no arrays) to typed ones, in order, nulls kept
CREATE OR REPLACE FUNCTION v2_jsonb_ints(j jsonb) RETURNS int[] IMMUTABLE LANGUAGE sql AS $$ SELECT COALESCE(array_agg(e::int ORDER BY i), '{}') FROM jsonb_array_elements_text(j) WITH ORDINALITY AS x(e, i) $$;
CREATE OR REPLACE FUNCTION v2_jsonb_reals(j jsonb) RETURNS real[] IMMUTABLE LANGUAGE sql AS $$ SELECT COALESCE(array_agg(e::real ORDER BY i), '{}') FROM jsonb_array_elements_text(j) WITH ORDINALITY AS x(e, i) $$;
CREATE OR REPLACE FUNCTION v2_jsonb_texts(j jsonb) RETURNS text[] IMMUTABLE LANGUAGE sql AS $$ SELECT COALESCE(array_agg(e ORDER BY i), '{}') FROM jsonb_array_elements_text(j) WITH ORDINALITY AS x(e, i) $$;
CREATE OR REPLACE FUNCTION v2_jsonb_times(j jsonb) RETURNS timestamp[] IMMUTABLE LANGUAGE sql AS $$ SELECT COALESCE(array_agg(e::timestamp ORDER BY i), '{}') FROM jsonb_array_elements_text(j) WITH ORDINALITY AS x(e, i) $$;
