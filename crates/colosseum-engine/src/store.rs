//! SQLite persistence for engines, tournaments and games.
//!
//! The schema is designed up front for **history + resume**: a tournament keeps a
//! full config snapshot and a status, and every game carries its status
//! (`pending`/`running`/`finished`/`discarded`), result, termination, nps, plies and
//! PGN. The `Store` is owned by a single scheduler task, so it needs no internal
//! locking.

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use colosseum_core::{
    EngineConfig, EngineId, GameId, GameResult, Termination, TournamentConfig, TournamentId,
};

use crate::error::EngineError;

type Result<T> = std::result::Result<T, EngineError>;

/// Lifecycle status of a tournament.
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_STOPPED: &str = "stopped";
pub const STATUS_FINISHED: &str = "finished";

/// Lifecycle status of a game.
pub const GAME_PENDING: &str = "pending";
pub const GAME_RUNNING: &str = "running";
pub const GAME_FINISHED: &str = "finished";
pub const GAME_DISCARDED: &str = "discarded";

/// A tournament row, as stored.
#[derive(Debug, Clone)]
pub struct TournamentRow {
    pub id: TournamentId,
    pub name: String,
    pub status: String,
    pub config: TournamentConfig,
    pub pgn_path: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// A game row, as stored.
#[derive(Debug, Clone)]
pub struct GameRow {
    pub id: GameId,
    pub round: u32,
    pub white: EngineId,
    pub black: EngineId,
    pub result: Option<GameResult>,
    pub termination: Option<Termination>,
    pub white_nps: Option<u64>,
    pub black_nps: Option<u64>,
    pub plies: Option<u32>,
    pub pgn: Option<String>,
    pub status: String,
}

/// One participating engine's seed and starting Elo within a tournament.
#[derive(Debug, Clone, Copy)]
pub struct TournamentEngineRow {
    pub engine: EngineId,
    pub seed: u32,
    pub start_elo: f64,
}

/// A SQLite-backed store.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) a store at `path`, applying the schema.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open an in-memory store (for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // ---- engines -------------------------------------------------------------

    /// Insert or replace an engine library entry.
    pub fn upsert_engine(&self, engine: &EngineConfig) -> Result<()> {
        let config_json = serde_json::to_string(engine)?;
        self.conn.execute(
            "INSERT INTO engines (id, name, version, elo, path, config_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name, version=excluded.version, elo=excluded.elo,
               path=excluded.path, config_json=excluded.config_json",
            params![
                engine.id.to_string(),
                engine.meta.name,
                engine.meta.version,
                engine.meta.elo,
                engine.path.to_string_lossy(),
                config_json,
            ],
        )?;
        Ok(())
    }

    /// Load all engine library entries.
    pub fn list_engines(&self) -> Result<Vec<EngineConfig>> {
        let mut stmt = self
            .conn
            .prepare("SELECT config_json FROM engines ORDER BY name")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut engines = Vec::new();
        for json in rows {
            engines.push(serde_json::from_str(&json?)?);
        }
        Ok(engines)
    }

    /// Delete an engine library entry.
    pub fn delete_engine(&self, id: EngineId) -> Result<()> {
        self.conn
            .execute("DELETE FROM engines WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    // ---- tournaments ---------------------------------------------------------

    /// Create a new tournament record (status `running`).
    pub fn create_tournament(
        &self,
        id: TournamentId,
        name: &str,
        config: &TournamentConfig,
    ) -> Result<()> {
        let config_json = serde_json::to_string(config)?;
        let pgn_path = config
            .pgn_output
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        self.conn.execute(
            "INSERT INTO tournaments (id, name, status, config_json, pgn_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                name,
                STATUS_RUNNING,
                config_json,
                pgn_path,
                now_iso8601(),
            ],
        )?;
        Ok(())
    }

    /// Update a tournament's status; sets `finished_at` when finishing.
    pub fn set_tournament_status(&self, id: TournamentId, status: &str) -> Result<()> {
        if status == STATUS_FINISHED {
            self.conn.execute(
                "UPDATE tournaments SET status = ?2, finished_at = ?3 WHERE id = ?1",
                params![id.to_string(), status, now_iso8601()],
            )?;
        } else {
            self.conn.execute(
                "UPDATE tournaments SET status = ?2 WHERE id = ?1",
                params![id.to_string(), status],
            )?;
        }
        Ok(())
    }

    /// List tournaments, most recent first (for the future History tab).
    pub fn list_tournaments(&self) -> Result<Vec<TournamentRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, status, config_json, pgn_path, created_at, finished_at
             FROM tournaments ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut tournaments = Vec::new();
        for row in rows {
            let (id, name, status, config_json, pgn_path, created_at, finished_at) = row?;
            tournaments.push(TournamentRow {
                id: TournamentId(parse_uuid(&id)?),
                name,
                status,
                config: serde_json::from_str(&config_json)?,
                pgn_path,
                created_at,
                finished_at,
            });
        }
        Ok(tournaments)
    }

    /// Load a single tournament by id.
    pub fn load_tournament(&self, id: TournamentId) -> Result<Option<TournamentRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, status, config_json, pgn_path, created_at, finished_at
                 FROM tournaments WHERE id = ?1",
                params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, name, status, config_json, pgn_path, created_at, finished_at)) = row else {
            return Ok(None);
        };
        Ok(Some(TournamentRow {
            id: TournamentId(parse_uuid(&id)?),
            name,
            status,
            config: serde_json::from_str(&config_json)?,
            pgn_path,
            created_at,
            finished_at,
        }))
    }

    /// Delete a tournament and its games (cascade).
    pub fn delete_tournament(&self, id: TournamentId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM tournaments WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // ---- participants --------------------------------------------------------

    /// Register a participating engine with its seed and starting Elo.
    pub fn add_tournament_engine(
        &self,
        tournament: TournamentId,
        engine: EngineId,
        seed: u32,
        start_elo: f64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tournament_engines (tournament_id, engine_id, seed, start_elo)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(tournament_id, engine_id) DO UPDATE SET
               seed=excluded.seed, start_elo=excluded.start_elo",
            params![tournament.to_string(), engine.to_string(), seed, start_elo],
        )?;
        Ok(())
    }

    /// List the participating engines of a tournament.
    pub fn list_tournament_engines(
        &self,
        tournament: TournamentId,
    ) -> Result<Vec<TournamentEngineRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT engine_id, seed, start_elo FROM tournament_engines
             WHERE tournament_id = ?1 ORDER BY seed",
        )?;
        let rows = stmt.query_map(params![tournament.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        let mut engines = Vec::new();
        for row in rows {
            let (engine, seed, start_elo) = row?;
            engines.push(TournamentEngineRow {
                engine: EngineId(parse_uuid(&engine)?),
                seed,
                start_elo,
            });
        }
        Ok(engines)
    }

    // ---- games ---------------------------------------------------------------

    /// Insert a pending game.
    pub fn insert_pending_game(
        &self,
        id: GameId,
        tournament: TournamentId,
        round: u32,
        white: EngineId,
        black: EngineId,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO games (id, tournament_id, round, white_id, black_id, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                tournament.to_string(),
                round,
                white.to_string(),
                black.to_string(),
                GAME_PENDING,
            ],
        )?;
        Ok(())
    }

    /// Mark a game as running.
    pub fn mark_game_running(&self, id: GameId) -> Result<()> {
        self.set_game_status(id, GAME_RUNNING)
    }

    /// Mark a game as discarded (e.g. aborted by Force-Stop).
    pub fn discard_game(&self, id: GameId) -> Result<()> {
        self.set_game_status(id, GAME_DISCARDED)
    }

    fn set_game_status(&self, id: GameId, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET status = ?2 WHERE id = ?1",
            params![id.to_string(), status],
        )?;
        Ok(())
    }

    /// Record a finished game's full outcome.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_game(
        &self,
        id: GameId,
        result: GameResult,
        termination: Termination,
        white_nps: Option<u64>,
        black_nps: Option<u64>,
        plies: u32,
        pgn: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET status = ?2, result = ?3, termination = ?4,
               white_nps = ?5, black_nps = ?6, plies = ?7, pgn = ?8, finished_at = ?9
             WHERE id = ?1",
            params![
                id.to_string(),
                GAME_FINISHED,
                serde_json::to_string(&result)?,
                serde_json::to_string(&termination)?,
                white_nps.map(|n| n as i64),
                black_nps.map(|n| n as i64),
                plies,
                pgn,
                now_iso8601(),
            ],
        )?;
        Ok(())
    }

    /// List all games of a tournament, ordered by round.
    pub fn list_games(&self, tournament: TournamentId) -> Result<Vec<GameRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, round, white_id, black_id, result, termination,
                    white_nps, black_nps, plies, pgn, status
             FROM games WHERE tournament_id = ?1 ORDER BY round, rowid",
        )?;
        let rows = stmt.query_map(params![tournament.to_string()], |row| {
            Ok(RawGame {
                id: row.get(0)?,
                round: row.get(1)?,
                white: row.get(2)?,
                black: row.get(3)?,
                result: row.get(4)?,
                termination: row.get(5)?,
                white_nps: row.get(6)?,
                black_nps: row.get(7)?,
                plies: row.get(8)?,
                pgn: row.get(9)?,
                status: row.get(10)?,
            })
        })?;
        let mut games = Vec::new();
        for row in rows {
            games.push(row?.into_game()?);
        }
        Ok(games)
    }
}

/// Intermediate row before parsing ids/enums.
struct RawGame {
    id: String,
    round: u32,
    white: String,
    black: String,
    result: Option<String>,
    termination: Option<String>,
    white_nps: Option<i64>,
    black_nps: Option<i64>,
    plies: Option<u32>,
    pgn: Option<String>,
    status: String,
}

impl RawGame {
    fn into_game(self) -> Result<GameRow> {
        Ok(GameRow {
            id: GameId(parse_uuid(&self.id)?),
            round: self.round,
            white: EngineId(parse_uuid(&self.white)?),
            black: EngineId(parse_uuid(&self.black)?),
            result: self.result.map(|s| serde_json::from_str(&s)).transpose()?,
            termination: self
                .termination
                .map(|s| serde_json::from_str(&s))
                .transpose()?,
            white_nps: self.white_nps.map(|n| n as u64),
            black_nps: self.black_nps.map(|n| n as u64),
            plies: self.plies,
            pgn: self.pgn,
            status: self.status,
        })
    }
}

fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| EngineError::Corrupt(format!("invalid uuid {s:?}: {e}")))
}

/// Current UTC time as a simple ISO-8601 string (`YYYY-MM-DDTHH:MM:SSZ`).
fn now_iso8601() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS engines (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  version     TEXT,
  elo         INTEGER,
  path        TEXT NOT NULL,
  config_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tournaments (
  id          TEXT PRIMARY KEY,
  name        TEXT,
  status      TEXT NOT NULL,
  config_json TEXT NOT NULL,
  pgn_path    TEXT,
  created_at  TEXT NOT NULL,
  finished_at TEXT
);

CREATE TABLE IF NOT EXISTS tournament_engines (
  tournament_id TEXT NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
  engine_id     TEXT NOT NULL,
  seed          INTEGER,
  start_elo     REAL,
  PRIMARY KEY (tournament_id, engine_id)
);

CREATE TABLE IF NOT EXISTS games (
  id            TEXT PRIMARY KEY,
  tournament_id TEXT NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
  round         INTEGER NOT NULL,
  white_id      TEXT NOT NULL,
  black_id      TEXT NOT NULL,
  result        TEXT,
  termination   TEXT,
  white_nps     INTEGER,
  black_nps     INTEGER,
  plies         INTEGER,
  pgn           TEXT,
  started_at    TEXT,
  finished_at   TEXT,
  status        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_games_tournament ON games(tournament_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    use colosseum_core::{GameResult, Termination, TournamentConfig};

    #[test]
    fn round_trips_engines_tournaments_and_games() {
        let store = Store::open_in_memory().unwrap();

        let mut engine = EngineConfig::new("stockfish".into());
        engine.meta.name = "Stockfish".into();
        engine.meta.elo = Some(3600);
        store.upsert_engine(&engine).unwrap();
        let loaded = store.list_engines().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].meta.name, "Stockfish");

        let tid = TournamentId::new();
        let config = TournamentConfig::default();
        store.create_tournament(tid, "Test", &config).unwrap();
        store
            .add_tournament_engine(tid, engine.id, 0, 3600.0)
            .unwrap();

        let other = EngineId::new();
        let gid = GameId::new();
        store
            .insert_pending_game(gid, tid, 1, engine.id, other)
            .unwrap();
        store.mark_game_running(gid).unwrap();
        store
            .finish_game(
                gid,
                GameResult::WhiteWin,
                Termination::Checkmate,
                Some(1_234_567),
                Some(2_345_678),
                42,
                "[Event \"Test\"]\n\n1. e4 e5 1-0",
            )
            .unwrap();

        let games = store.list_games(tid).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].result, Some(GameResult::WhiteWin));
        assert_eq!(games[0].termination, Some(Termination::Checkmate));
        assert_eq!(games[0].plies, Some(42));
        assert_eq!(games[0].status, GAME_FINISHED);

        store.set_tournament_status(tid, STATUS_FINISHED).unwrap();
        let row = store.load_tournament(tid).unwrap().unwrap();
        assert_eq!(row.status, STATUS_FINISHED);
        assert!(row.finished_at.is_some());

        let participants = store.list_tournament_engines(tid).unwrap();
        assert_eq!(participants.len(), 1);
        assert!((participants[0].start_elo - 3600.0).abs() < f64::EPSILON);

        store.delete_tournament(tid).unwrap();
        assert!(store.load_tournament(tid).unwrap().is_none());
        assert!(store.list_games(tid).unwrap().is_empty());
    }
}
