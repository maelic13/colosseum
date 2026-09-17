//! The `book` command: hash, stats, verify and deterministic slice.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct BookCommand {
    #[command(subcommand)]
    pub(crate) action: BookAction,
}

#[derive(Debug, Subcommand)]
pub(crate) enum BookAction {
    /// Write a deterministic canonical EPD subset.
    Slice(BookSliceCommand),
    /// Compute the SHA-256 of the exact input bytes.
    Hash(BookInput),
    /// Report parsed entry, uniqueness and ply statistics.
    Stats(BookInput),
    /// Strictly account for every candidate and reject malformed entries.
    Verify(BookInput),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct BookInput {
    /// EPD or PGN input path.
    pub(crate) input: PathBuf,
    /// Override format detection from the file extension.
    #[arg(long, value_enum)]
    pub(crate) format: Option<BookFormatArg>,
    /// PGN half-moves retained per game.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) plies: u32,
}

#[derive(Debug, Args)]
pub(crate) struct BookSliceCommand {
    #[command(flatten)]
    pub(crate) book: BookInput,
    /// Canonical EPD output path.
    pub(crate) output: PathBuf,
    /// Number of ordered entries skipped before writing.
    #[arg(long, default_value_t = 0)]
    pub(crate) start: usize,
    /// Maximum entries written.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) count: u64,
    /// Sequential file order or named-stream random order.
    #[arg(long, value_enum, default_value_t = BookOrderArg::Sequential)]
    pub(crate) order: BookOrderArg,
    /// Master seed used by random order.
    #[arg(long, default_value_t = 0)]
    pub(crate) seed: u64,
    /// Permit replacing an existing output file.
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum BookFormatArg {
    Epd,
    Pgn,
}

impl From<BookFormatArg> for OpeningFormat {
    fn from(value: BookFormatArg) -> Self {
        match value {
            BookFormatArg::Epd => Self::Epd,
            BookFormatArg::Pgn => Self::Pgn,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BookHashReport {
    pub(crate) path: PathBuf,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BookStatsReport {
    pub(crate) path: PathBuf,
    pub(crate) format: String,
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
    pub(crate) candidates: usize,
    pub(crate) usable: usize,
    pub(crate) rejected: usize,
    pub(crate) unique: usize,
    pub(crate) duplicates: usize,
    pub(crate) min_plies: usize,
    pub(crate) max_plies: usize,
    pub(crate) mean_plies: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) eval_band: Option<BookEvalBand>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BookEvalBand {
    pub(crate) samples: usize,
    pub(crate) unit: String,
    pub(crate) minimum: f64,
    pub(crate) mean: f64,
    pub(crate) maximum: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BookSliceReport {
    pub(crate) input: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) input_sha256: String,
    pub(crate) output_sha256: String,
    pub(crate) start: usize,
    pub(crate) requested_count: usize,
    pub(crate) written: usize,
    pub(crate) order: String,
    pub(crate) seed: u64,
}

pub(crate) fn run_book(action: BookAction, machine: bool) -> ExitCode {
    match action {
        BookAction::Hash(input) => match hash_file_report(&input.input) {
            Ok(report) => {
                if machine {
                    print_json(&MachineOutput::BookHash { report });
                } else {
                    println!("{}  {}", report.sha256, report.path.display());
                    println!("bytes: {}", report.bytes);
                }
                ExitCode::SUCCESS
            }
            Err(error) => book_error(error),
        },
        BookAction::Verify(input) => match opening_book(&input)
            .and_then(|book| audit_opening_book(&book).map_err(|error| error.to_string()))
        {
            Ok(audit) => {
                let valid = audit.valid();
                if machine {
                    print_json(&MachineOutput::BookVerify { audit });
                } else {
                    println!(
                        "{}: {} usable of {} candidates",
                        if valid { "valid" } else { "invalid" },
                        audit.usable,
                        audit.candidates
                    );
                    if !audit.rejected_indices.is_empty() {
                        println!(
                            "rejected candidate indices: {}",
                            audit
                                .rejected_indices
                                .iter()
                                .map(usize::to_string)
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                    }
                }
                if valid {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => book_error(error),
        },
        BookAction::Stats(input) => match book_stats(&input) {
            Ok(report) => {
                if machine {
                    print_json(&MachineOutput::BookStats { report });
                } else {
                    println!(
                        "{}: {} usable / {} candidates; {} unique, {} duplicates",
                        report.format,
                        report.usable,
                        report.candidates,
                        report.unique,
                        report.duplicates
                    );
                    println!(
                        "plies: min {}, mean {:.3}, max {}",
                        report.min_plies, report.mean_plies, report.max_plies
                    );
                    if let Some(eval) = &report.eval_band {
                        println!(
                            "eval band ({}; {} samples): {:.3}..{:.3}, mean {:.3}",
                            eval.unit, eval.samples, eval.minimum, eval.maximum, eval.mean
                        );
                    }
                    println!("SHA-256: {}", report.sha256);
                }
                ExitCode::SUCCESS
            }
            Err(error) => book_error(error),
        },
        BookAction::Slice(command) => match slice_book(&command) {
            Ok(report) => {
                if machine {
                    print_json(&MachineOutput::BookSlice { report });
                } else {
                    println!(
                        "wrote {} canonical EPD entries to {}",
                        report.written,
                        report.output.display()
                    );
                    println!("output SHA-256: {}", report.output_sha256);
                }
                ExitCode::SUCCESS
            }
            Err(error) => book_error(error),
        },
    }
}

pub(crate) fn opening_book(input: &BookInput) -> Result<OpeningBook, String> {
    let format = input.format.map(Into::into).unwrap_or_else(|| {
        OpeningFormat::from_extension(
            input
                .input
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
        )
    });
    Ok(OpeningBook {
        path: input.input.clone(),
        format,
        order: OpeningOrder::Sequential,
        count: None,
        plies: input.plies,
        seed: 0,
    })
}

pub(crate) fn hash_file_report(path: &Path) -> Result<BookHashReport, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read book {}: {error}", path.display()))?;
    Ok(BookHashReport {
        path: path.to_owned(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

pub(crate) fn book_stats(input: &BookInput) -> Result<BookStatsReport, String> {
    let book = opening_book(input)?;
    let audit = audit_opening_book(&book).map_err(|error| error.to_string())?;
    let openings = load_openings_named(&book, 0).map_err(|error| error.to_string())?;
    let hash = hash_file_report(&input.input)?;
    let unique = openings
        .iter()
        .map(|opening| format!("{:?}\0{}", opening.start_fen, opening.moves.join(" ")))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let min_plies = openings
        .iter()
        .map(|opening| opening.moves.len())
        .min()
        .unwrap_or(0);
    let max_plies = openings
        .iter()
        .map(|opening| opening.moves.len())
        .max()
        .unwrap_or(0);
    let mean_plies = openings
        .iter()
        .map(|opening| opening.moves.len() as f64)
        .sum::<f64>()
        / openings.len() as f64;
    let text = fs::read_to_string(&input.input)
        .map_err(|error| format!("cannot read book {}: {error}", input.input.display()))?;
    let (eval_values, eval_unit) = book_eval_values(&text, book.format);
    let eval_band = (!eval_values.is_empty()).then(|| BookEvalBand {
        samples: eval_values.len(),
        unit: eval_unit.into(),
        minimum: eval_values.iter().copied().fold(f64::INFINITY, f64::min),
        mean: eval_values.iter().sum::<f64>() / eval_values.len() as f64,
        maximum: eval_values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max),
    });
    Ok(BookStatsReport {
        path: input.input.clone(),
        format: book.format.label().into(),
        sha256: hash.sha256,
        bytes: hash.bytes,
        candidates: audit.candidates,
        usable: audit.usable,
        rejected: audit.rejected_indices.len(),
        unique,
        duplicates: openings.len() - unique,
        min_plies,
        max_plies,
        mean_plies,
        eval_band,
    })
}

pub(crate) fn book_eval_values(text: &str, format: OpeningFormat) -> (Vec<f64>, &'static str) {
    match format {
        OpeningFormat::Epd => {
            let values = text
                .lines()
                .filter_map(|line| {
                    let tokens = line
                        .split(|character: char| character.is_whitespace() || character == ';')
                        .filter(|token| !token.is_empty())
                        .collect::<Vec<_>>();
                    tokens
                        .windows(2)
                        .find_map(|pair| (pair[0] == "ce").then(|| pair[1].parse().ok()).flatten())
                })
                .collect();
            (values, "centipawns (EPD ce)")
        }
        OpeningFormat::Pgn => {
            let mut rest = text;
            let mut values = Vec::new();
            while let Some((_, after)) = rest.split_once("[%eval ") {
                if let Some(value) = after
                    .split(|character: char| character == ']' || character.is_whitespace())
                    .next()
                    .and_then(|value| value.parse().ok())
                {
                    values.push(value);
                }
                rest = after;
            }
            (values, "pawns (PGN %eval)")
        }
    }
}

pub(crate) fn slice_book(command: &BookSliceCommand) -> Result<BookSliceReport, String> {
    if command.output == command.book.input {
        return Err("slice output must differ from the input path".into());
    }
    let mut book = opening_book(&command.book)?;
    let audit = audit_opening_book(&book).map_err(|error| error.to_string())?;
    if !audit.valid() {
        return Err(format!(
            "book contains rejected candidates at indices {}",
            audit
                .rejected_indices
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    book.order = match command.order {
        BookOrderArg::Sequential => OpeningOrder::Sequential,
        BookOrderArg::Random => OpeningOrder::Random,
    };
    let openings = load_openings_named(&book, command.seed).map_err(|error| error.to_string())?;
    if command.start >= openings.len() {
        return Err(format!(
            "slice start {} is outside {} usable entries",
            command.start,
            openings.len()
        ));
    }
    let selected = openings
        .iter()
        .skip(command.start)
        .take(usize::try_from(command.count).unwrap_or(usize::MAX))
        .collect::<Vec<_>>();
    let mut output = String::new();
    for opening in &selected {
        let fen = fen_after(opening.start_fen.as_deref(), &opening.moves)
            .ok_or_else(|| format!("cannot materialize opening {:?}", opening.label))?;
        output.push_str(&fen.split_whitespace().take(4).collect::<Vec<_>>().join(" "));
        output.push('\n');
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if command.force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(&command.output).map_err(|error| {
        format!(
            "cannot create slice output {}: {error}",
            command.output.display()
        )
    })?;
    file.write_all(output.as_bytes()).map_err(|error| {
        format!(
            "cannot write slice output {}: {error}",
            command.output.display()
        )
    })?;
    file.sync_all().map_err(|error| {
        format!(
            "cannot flush slice output {}: {error}",
            command.output.display()
        )
    })?;
    let input_hash = hash_file_report(&command.book.input)?;
    let output_hash = hash_file_report(&command.output)?;
    Ok(BookSliceReport {
        input: command.book.input.clone(),
        output: command.output.clone(),
        input_sha256: input_hash.sha256,
        output_sha256: output_hash.sha256,
        start: command.start,
        requested_count: usize::try_from(command.count).unwrap_or(usize::MAX),
        written: selected.len(),
        order: match command.order {
            BookOrderArg::Sequential => "sequential",
            BookOrderArg::Random => "random",
        }
        .into(),
        seed: command.seed,
    })
}

pub(crate) fn book_error(error: String) -> ExitCode {
    eprintln!("book command failed: {error}");
    ExitCode::FAILURE
}
