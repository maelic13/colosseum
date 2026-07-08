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
    /// Mean search depth over each engine's moves in this game.
    pub white_depth: Option<f64>,
    pub black_depth: Option<f64>,
    /// Mean wall-clock milliseconds per move for each engine in this game.
    pub white_move_ms: Option<f64>,
    pub black_move_ms: Option<f64>,
    pub plies: Option<u32>,
    pub pgn: Option<String>,
    pub status: String,
    /// Opening start FEN; `None` means the standard start position.
    pub start_fen: Option<String>,
    /// Opening moves (UCI) pre-played before the engines move.
    pub opening_moves: Vec<String>,
}

/// One game of a schedule, for bulk insertion via
/// [`Store::insert_pending_games`].
#[derive(Debug, Clone, Copy)]
pub struct PendingGame<'a> {
    pub id: GameId,
    pub round: u32,
    pub white: EngineId,
    pub black: EngineId,
    /// Opening start FEN; `None` means the standard start position.
    pub start_fen: Option<&'a str>,
    /// Opening moves (UCI) pre-played before the engines move.
    pub opening_moves: &'a [String],
}

/// One participating engine's seed, starting Elo, and config snapshot within a tournament.
#[derive(Debug, Clone)]
pub struct TournamentEngineRow {
    pub engine: EngineId,
    pub seed: u32,
    pub start_elo: f64,
    /// Full engine config snapshot stored at tournament creation.
    /// `None` for databases created before Step 6 that lack the column.
    pub config: Option<EngineConfig>,
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
        // GUI and driver connections write concurrently; without a busy
        // timeout a write that lands mid-transaction fails instantly with
        // SQLITE_BUSY (seen as bulk-delete skipping the running tournament).
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        // Migration: add `engine_config_json` to `tournament_engines` if absent.
        // This column was introduced in Step 6; databases from Step 5 lack it.
        let has_col: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('tournament_engines') \
                 WHERE name='engine_config_json'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_col {
            conn.execute_batch(
                "ALTER TABLE tournament_engines ADD COLUMN engine_config_json TEXT;",
            )?;
        }
        // Migration: per-game opening columns, introduced in Step 10.
        for column in [
            "start_fen TEXT",
            "opening_moves TEXT",
            "white_depth REAL",
            "black_depth REAL",
            "white_move_ms REAL",
            "black_move_ms REAL",
        ] {
            let name = column.split_whitespace().next().unwrap();
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('games') WHERE name = ?1",
                    params![name],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;
            if !exists {
                conn.execute_batch(&format!("ALTER TABLE games ADD COLUMN {column};"))?;
            }
        }
        Ok(Self { conn })
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

    /// Overwrite a tournament's stored config (e.g. a live concurrency change,
    /// so a later resume uses the new value).
    pub fn update_tournament_config(
        &self,
        id: TournamentId,
        config: &TournamentConfig,
    ) -> Result<()> {
        let config_json = serde_json::to_string(config)?;
        self.conn.execute(
            "UPDATE tournaments SET config_json = ?2 WHERE id = ?1",
            params![id.to_string(), config_json],
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

    /// Register a participating engine with its seed, starting Elo, and config snapshot.
    pub fn add_tournament_engine(
        &self,
        tournament: TournamentId,
        engine_id: EngineId,
        engine_config: &EngineConfig,
        seed: u32,
        start_elo: f64,
    ) -> Result<()> {
        let config_json = serde_json::to_string(engine_config)?;
        self.conn.execute(
            "INSERT INTO tournament_engines
               (tournament_id, engine_id, seed, start_elo, engine_config_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(tournament_id, engine_id) DO UPDATE SET
               seed=excluded.seed, start_elo=excluded.start_elo,
               engine_config_json=excluded.engine_config_json",
            params![
                tournament.to_string(),
                engine_id.to_string(),
                seed,
                start_elo,
                config_json,
            ],
        )?;
        Ok(())
    }

    /// List the participating engines of a tournament, ordered by insertion seed.
    pub fn list_tournament_engines(
        &self,
        tournament: TournamentId,
    ) -> Result<Vec<TournamentEngineRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT engine_id, seed, start_elo, engine_config_json
             FROM tournament_engines
             WHERE tournament_id = ?1 ORDER BY seed",
        )?;
        let rows = stmt.query_map(params![tournament.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut engines = Vec::new();
        for row in rows {
            let (engine, seed, start_elo, config_json) = row?;
            let config = config_json
                .map(|json| serde_json::from_str::<EngineConfig>(&json))
                .transpose()?;
            engines.push(TournamentEngineRow {
                engine: EngineId(parse_uuid(&engine)?),
                seed,
                start_elo,
                config,
            });
        }
        Ok(engines)
    }

    // ---- games ---------------------------------------------------------------

    /// Insert a whole schedule of pending games in **one transaction**.
    ///
    /// Inserting thousands of rows through [`Self::insert_pending_game`] pays
    /// one commit (and disk sync) per row — minutes for a big tournament and
    /// a frozen UI. One transaction does the same work in milliseconds.
    pub fn insert_pending_games(
        &self,
        tournament: TournamentId,
        games: &[PendingGame<'_>],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO games
                   (id, tournament_id, round, white_id, black_id, status, start_fen, opening_moves)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for game in games {
                let moves_json = if game.opening_moves.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(game.opening_moves)?)
                };
                stmt.execute(params![
                    game.id.to_string(),
                    tournament.to_string(),
                    game.round,
                    game.white.to_string(),
                    game.black.to_string(),
                    GAME_PENDING,
                    game.start_fen,
                    moves_json,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Reset every non-finished game of a tournament back to pending in a
    /// single statement (resume path). Returns how many rows changed.
    pub fn reset_unfinished_games(&self, tournament: TournamentId) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE games
             SET status = ?2, result = NULL, termination = NULL,
                 white_nps = NULL, black_nps = NULL, plies = NULL, pgn = NULL,
                 white_depth = NULL, black_depth = NULL,
                 white_move_ms = NULL, black_move_ms = NULL,
                 started_at = NULL, finished_at = NULL
             WHERE tournament_id = ?1 AND status IN (?3, ?4)",
            params![
                tournament.to_string(),
                GAME_PENDING,
                GAME_RUNNING,
                GAME_DISCARDED
            ],
        )?)
    }

    /// Insert a pending game with its assigned opening (start FEN + pre-moves).
    #[allow(clippy::too_many_arguments)]
    pub fn insert_pending_game(
        &self,
        id: GameId,
        tournament: TournamentId,
        round: u32,
        white: EngineId,
        black: EngineId,
        start_fen: Option<&str>,
        opening_moves: &[String],
    ) -> Result<()> {
        let moves_json = if opening_moves.is_empty() {
            None
        } else {
            Some(serde_json::to_string(opening_moves)?)
        };
        self.conn.execute(
            "INSERT INTO games
               (id, tournament_id, round, white_id, black_id, status, start_fen, opening_moves)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id.to_string(),
                tournament.to_string(),
                round,
                white.to_string(),
                black.to_string(),
                GAME_PENDING,
                start_fen,
                moves_json,
            ],
        )?;
        Ok(())
    }

    /// Mark a game as running, recording the wall-clock start time.
    pub fn mark_game_running(&self, id: GameId) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET status = ?2, started_at = ?3 WHERE id = ?1",
            params![id.to_string(), GAME_RUNNING, now_iso8601()],
        )?;
        Ok(())
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
        white_depth: Option<f64>,
        black_depth: Option<f64>,
        white_move_ms: Option<f64>,
        black_move_ms: Option<f64>,
        plies: u32,
        pgn: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET status = ?2, result = ?3, termination = ?4,
               white_nps = ?5, black_nps = ?6, plies = ?7, pgn = ?8, finished_at = ?9,
               white_depth = ?10, black_depth = ?11, white_move_ms = ?12, black_move_ms = ?13
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
                white_depth,
                black_depth,
                white_move_ms,
                black_move_ms,
            ],
        )?;
        Ok(())
    }

    /// List all games of a tournament, ordered by round.
    pub fn list_games(&self, tournament: TournamentId) -> Result<Vec<GameRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, round, white_id, black_id, result, termination,
                    white_nps, black_nps, plies, pgn, status, start_fen, opening_moves,
                    white_depth, black_depth, white_move_ms, black_move_ms
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
                start_fen: row.get(11)?,
                opening_moves: row.get(12)?,
                white_depth: row.get(13)?,
                black_depth: row.get(14)?,
                white_move_ms: row.get(15)?,
                black_move_ms: row.get(16)?,
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
    white_depth: Option<f64>,
    black_depth: Option<f64>,
    white_move_ms: Option<f64>,
    black_move_ms: Option<f64>,
    plies: Option<u32>,
    pgn: Option<String>,
    status: String,
    start_fen: Option<String>,
    opening_moves: Option<String>,
}

impl RawGame {
    fn into_game(self) -> Result<GameRow> {
        let opening_moves = self
            .opening_moves
            .map(|s| serde_json::from_str::<Vec<String>>(&s))
            .transpose()?
            .unwrap_or_default();
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
            white_depth: self.white_depth,
            black_depth: self.black_depth,
            white_move_ms: self.white_move_ms,
            black_move_ms: self.black_move_ms,
            plies: self.plies,
            pgn: self.pgn,
            status: self.status,
            start_fen: self.start_fen,
            opening_moves,
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
  tournament_id      TEXT NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
  engine_id          TEXT NOT NULL,
  seed               INTEGER,
  start_elo          REAL,
  engine_config_json TEXT,
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
  white_depth   REAL,
  black_depth   REAL,
  white_move_ms REAL,
  black_move_ms REAL,
  plies         INTEGER,
  pgn           TEXT,
  started_at    TEXT,
  finished_at   TEXT,
  status        TEXT NOT NULL,
  start_fen     TEXT,
  opening_moves TEXT
);

CREATE INDEX IF NOT EXISTS idx_games_tournament ON games(tournament_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    use colosseum_core::{GameResult, Termination, TournamentConfig};

    #[test]
    fn bulk_insert_and_reset_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let tid = TournamentId::new();
        store
            .create_tournament(tid, "Bulk", &TournamentConfig::default())
            .unwrap();

        let (a, b) = (EngineId::new(), EngineId::new());
        let opening = vec!["e2e4".to_string(), "e7e5".to_string()];
        let games: Vec<(GameId, u32)> = (0..500).map(|i| (GameId::new(), i / 2 + 1)).collect();
        let rows: Vec<PendingGame<'_>> = games
            .iter()
            .map(|(id, round)| PendingGame {
                id: *id,
                round: *round,
                white: a,
                black: b,
                start_fen: None,
                opening_moves: &opening,
            })
            .collect();
        store.insert_pending_games(tid, &rows).unwrap();

        let listed = store.list_games(tid).unwrap();
        assert_eq!(listed.len(), 500);
        assert_eq!(listed[0].opening_moves, opening);

        // Finish one, leave one running, one discarded; reset touches only
        // the non-finished ones.
        store.mark_game_running(games[0].0).unwrap();
        store
            .finish_game(
                games[0].0,
                GameResult::WhiteWin,
                Termination::Checkmate,
                None,
                None,
                None,
                None,
                None,
                None,
                10,
                "pgn",
            )
            .unwrap();
        store.mark_game_running(games[1].0).unwrap();
        store.mark_game_running(games[2].0).unwrap();
        store.discard_game(games[2].0).unwrap();

        let reset = store.reset_unfinished_games(tid).unwrap();
        assert_eq!(reset, 2); // the running + discarded ones
        let listed = store.list_games(tid).unwrap();
        assert!(listed.iter().filter(|g| g.status == GAME_FINISHED).count() == 1);
        assert!(
            listed
                .iter()
                .filter(|g| g.status == GAME_PENDING)
                .count()
                == 499
        );
    }

    #[test]
    fn round_trips_tournaments_and_games() {
        let store = Store::open_in_memory().unwrap();

        let mut engine = EngineConfig::new("stockfish".into());
        engine.meta.name = "Stockfish".into();
        engine.meta.elo = Some(3600);

        let tid = TournamentId::new();
        let config = TournamentConfig::default();
        store.create_tournament(tid, "Test", &config).unwrap();
        store
            .add_tournament_engine(tid, engine.id, &engine, 0, 3600.0)
            .unwrap();

        let other = EngineId::new();
        let gid = GameId::new();
        store
            .insert_pending_game(gid, tid, 1, engine.id, other, None, &[])
            .unwrap();
        store.mark_game_running(gid).unwrap();
        store
            .finish_game(
                gid,
                GameResult::WhiteWin,
                Termination::Checkmate,
                Some(1_234_567),
                Some(2_345_678),
                Some(21.5),
                Some(18.0),
                Some(120.0),
                Some(95.5),
                42,
                "[Event \"Test\"]\n\n1. e4 e5 1-0",
            )
            .unwrap();

        // A second game carrying an assigned opening (start FEN + pre-moves).
        let gid2 = GameId::new();
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
        store
            .insert_pending_game(
                gid2,
                tid,
                1,
                other,
                engine.id,
                Some(fen),
                &["e2e4".to_string(), "e7e5".to_string()],
            )
            .unwrap();

        let games = store.list_games(tid).unwrap();
        assert_eq!(games.len(), 2);
        let g1 = games.iter().find(|g| g.id == gid).unwrap();
        assert_eq!(g1.result, Some(GameResult::WhiteWin));
        assert_eq!(g1.termination, Some(Termination::Checkmate));
        assert_eq!(g1.plies, Some(42));
        assert_eq!(g1.white_nps, Some(1_234_567));
        assert_eq!(g1.white_depth, Some(21.5));
        assert_eq!(g1.black_depth, Some(18.0));
        assert_eq!(g1.white_move_ms, Some(120.0));
        assert_eq!(g1.black_move_ms, Some(95.5));
        assert_eq!(g1.status, GAME_FINISHED);
        assert_eq!(g1.start_fen, None);
        assert!(g1.opening_moves.is_empty());

        let g2 = games.iter().find(|g| g.id == gid2).unwrap();
        assert_eq!(g2.start_fen.as_deref(), Some(fen));
        assert_eq!(g2.opening_moves, vec!["e2e4", "e7e5"]);

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
