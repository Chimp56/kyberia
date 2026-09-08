mod observation_export;

use kyberia_domain::identity::ProjectId;
use kyberia_project_store::{Bundle, OpenMode};
use std::path::Path;
use std::process::ExitCode;

fn run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.is_empty() || args == ["--help"] || args == ["help"] {
        println!(
            "Kyberia project CLI\n\n  kyberia new <directory.rfatlas> <name>\n  kyberia inspect <directory.rfatlas>\n  kyberia verify <directory.rfatlas>\n  kyberia recover-manifest <directory.rfatlas>\n  kyberia export-observations-parquet <directory.rfatlas> <new-directory>\n\nOutputs are JSON. verify exits nonzero for integrity failures. Exports never overwrite an existing destination."
        );
        return Ok(true);
    }
    if args == ["--version"] {
        println!("kyberia {}", env!("CARGO_PKG_VERSION"));
        return Ok(true);
    }
    match args.first().map(String::as_str) {
        Some("new") if args.len() == 3 => {
            let id = ProjectId::from_bytes(*uuid::Uuid::new_v4().as_bytes())?;
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis(),
            )?;
            let bundle = Bundle::create(Path::new(&args[1]), id, args[2].clone(), now)?;
            println!("{}", serde_json::to_string_pretty(&bundle.manifest()?)?);
            Ok(true)
        }
        Some("inspect") if args.len() == 2 => {
            let bundle = Bundle::open(Path::new(&args[1]), OpenMode::ReadOnly)?;
            println!("{}", serde_json::to_string_pretty(&bundle.manifest()?)?);
            Ok(true)
        }
        Some("verify") if args.len() == 2 => {
            let result = Bundle::open(Path::new(&args[1]), OpenMode::ReadOnly)?.verify()?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(result.failures.is_empty())
        }
        Some("recover-manifest") if args.len() == 2 => {
            let bundle = Bundle::open(Path::new(&args[1]), OpenMode::ReadWrite)?;
            bundle.recover_manifest()?;
            let result = bundle.verify()?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(result.failures.is_empty())
        }
        Some("export-observations-parquet") if args.len() == 3 => {
            let result = observation_export::export(Path::new(&args[1]), Path::new(&args[2]))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(true)
        }
        _ => Err("invalid command or arguments; use kyberia --help".into()),
    }
}

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("{}", serde_json::json!({"error": error.to_string()}));
            ExitCode::from(2)
        }
    }
}
