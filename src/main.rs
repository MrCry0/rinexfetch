use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};

use rinexfetch::cddis::auth::CddisClient;
use rinexfetch::cddis::discovery;
use rinexfetch::error::RinexFetchError;
use rinexfetch::rinex_merge::{nav, obs};
use rinexfetch::secrets::CredentialProvider;
use rinexfetch::secrets::interactive::InteractiveCredentialProvider;
use rinexfetch::secrets::keyring::KeyringCredentialProvider;
use rinexfetch::systems::{self, GnssSystem};
use rinexfetch::time::GpsDay;

/// Fetch and combine RINEX data from NASA's CDDIS archive for a given time,
/// GNSS constellation set, and set of ground stations.
#[derive(Debug, Parser)]
#[command(name = "rinexfetch", version, about)]
struct Cli {
    /// `latest` or an explicit ISO 8601 timestamp.
    #[arg(long)]
    time: String,

    /// `all` or a comma-separated subset of gps,glonass,galileo,beidou,qzss,sbas.
    #[arg(long, default_value = "all")]
    systems: String,

    /// Comma-separated IGS station identifiers. Omitted or empty means
    /// nav-only mode (no obs files fetched).
    #[arg(long)]
    stations: Option<String>,

    /// Target RINEX output version: 3 or 4.
    #[arg(long, default_value_t = 4)]
    rinex_version: u8,

    #[arg(long)]
    output_dir: PathBuf,

    #[arg(long, value_enum, default_value_t = CredentialBackend::Interactive)]
    credential_provider: CredentialBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CredentialBackend {
    Interactive,
    Keyring,
}

fn parse_stations(raw: &Option<String>) -> Vec<String> {
    raw.as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn run(cli: Cli) -> Result<(), RinexFetchError> {
    if cli.rinex_version != 3 && cli.rinex_version != 4 {
        return Err(RinexFetchError::UnsupportedRinexVersion(cli.rinex_version));
    }

    let gps_day = GpsDay::resolve(&cli.time)?;
    let systems = systems::parse_systems(&cli.systems).map_err(RinexFetchError::InvalidSystems)?;
    let stations = parse_stations(&cli.stations);

    let provider: Box<dyn CredentialProvider> = match cli.credential_provider {
        CredentialBackend::Interactive => Box::new(InteractiveCredentialProvider),
        CredentialBackend::Keyring => Box::new(KeyringCredentialProvider),
    };
    let token = provider.token()?;

    let client = CddisClient::new(token.clone())?;
    client.verify_token()?;
    provider.on_verified(&token)?;

    print_summary(&gps_day, &systems, &stations, &cli.output_dir, &token);

    std::fs::create_dir_all(&cli.output_dir).map_err(RinexFetchError::OutputDir)?;

    let candidates = if cli.time == "latest" {
        discovery::nav_candidates_for_latest(gps_day)
    } else {
        discovery::nav_candidates_for_day(gps_day)
    };
    let nav_outcome = nav::fetch_and_write(
        &client,
        &candidates,
        &systems,
        cli.rinex_version,
        &cli.output_dir,
    )?;

    println!();
    println!(
        "Nav: wrote {} ({:?} tier, day {:04}-{:03}, RINEX {})",
        nav_outcome.output_path.display(),
        nav_outcome.tier,
        nav_outcome.day.year,
        nav_outcome.day.day_of_year,
        cli.rinex_version,
    );
    if nav_outcome.dropped_non_ephemeris > 0 {
        println!(
            "Note: {} non-ephemeris nav frame(s) (system time offset / earth \
             orientation / ionosphere model) were present in the source but are not \
             written to output — the rinex crate's nav writer currently only formats \
             ephemeris frames.",
            nav_outcome.dropped_non_ephemeris
        );
    }

    if !stations.is_empty() {
        println!();
        let outcomes = obs::fetch_and_write_all(
            &client,
            nav_outcome.day,
            &stations,
            &systems,
            cli.rinex_version,
            &cli.output_dir,
        );
        let failed = outcomes.iter().filter(|o| o.result.is_err()).count();
        for outcome in &outcomes {
            match &outcome.result {
                Ok(path) => println!("Obs [{}]: wrote {}", outcome.station, path.display()),
                Err(err) => println!("Obs [{}]: FAILED — {err}", outcome.station),
            }
        }
        if failed > 0 {
            println!(
                "{failed} of {} station(s) failed; see above (each is isolated and doesn't \
                 affect the others or the nav fetch).",
                outcomes.len()
            );
        }
    }

    Ok(())
}

fn print_summary(
    gps_day: &GpsDay,
    systems: &[GnssSystem],
    stations: &[String],
    output_dir: &Path,
    token: &str,
) {
    println!(
        "Resolved day: {:04}-{:03} (GPS week {}, day {} of GPS week)",
        gps_day.year, gps_day.day_of_year, gps_day.gps_week, gps_day.gps_day_of_week
    );

    let system_names: Vec<String> = systems.iter().map(GnssSystem::to_string).collect();
    println!("Systems: {}", system_names.join(", "));

    if stations.is_empty() {
        println!("Stations: none (nav-only mode)");
    } else {
        println!("Stations: {}", stations.join(", "));
    }

    println!("Output directory: {}", output_dir.display());
    println!(
        "Bearer token verified against CDDIS ({} chars, held in memory only)",
        token.len()
    );
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
