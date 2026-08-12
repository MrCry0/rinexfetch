use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};

use rinexfetch::cddis::auth::CddisClient;
use rinexfetch::error::RinexFetchError;
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
    /// `now`, `yesterday`, or an explicit ISO 8601 timestamp.
    #[arg(long)]
    time: String,

    /// `all` or a comma-separated subset of gps,glonass,galileo,beidou,qzss,sbas.
    #[arg(long, default_value = "all")]
    systems: String,

    /// Comma-separated IGS station identifiers. Omitted or empty means
    /// nav-only mode (no obs files fetched).
    #[arg(long)]
    stations: Option<String>,

    /// Target RINEX output version. Only 4 is currently supported.
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
    if cli.rinex_version != 4 {
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

    println!();
    println!(
        "Not yet implemented: CDDIS product discovery, download, and RINEX writing (see \
         rinexfetch-project-plan.md, Phases 3-4). This build resolves inputs and \
         authenticates against CDDIS (Phases 1-2)."
    );

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
