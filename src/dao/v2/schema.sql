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
