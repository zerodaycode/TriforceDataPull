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
