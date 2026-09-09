mod cancellation;
mod observation_export;
mod stored_analysis;

use kyberia_domain::identity::ProjectId;
use kyberia_project_store::{Bundle, Cancellation, NeverCancel, OpenMode};
use std::path::Path;
use std::process::ExitCode;

fn run(
    args: &[String],
    cancellation: &dyn Cancellation,
) -> Result<bool, Box<dyn std::error::Error>> {
    if args.is_empty() || args == ["--help"] || args == ["help"] {
        println!(
            "Kyberia project CLI\n\n  kyberia new <directory.rfatlas> <name>\n  kyberia inspect <directory.rfatlas>\n  kyberia verify <directory.rfatlas>\n  kyberia recover-manifest <directory.rfatlas>\n  kyberia export-observations-parquet <directory.rfatlas> <new-directory>\n  kyberia analyze-stored-rssi <directory.rfatlas> <request.json> <new-output-directory>\n  kyberia export-stored-rssi-scene <directory.rfatlas> <request.json> <new-output-directory>\n\nOutputs are JSON. verify exits nonzero for integrity failures. Exports and analysis outputs never overwrite an existing destination."
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
        Some("analyze-stored-rssi" | "export-stored-rssi-scene") if args.len() == 4 => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "analysis_started",
                    "cancellation": "sigint",
                })
            );
            let analyze = if args[0] == "export-stored-rssi-scene" {
                stored_analysis::scene_with_cancellation
            } else {
                stored_analysis::analyze_with_cancellation
            };
            let result = analyze(
                Path::new(&args[1]),
                Path::new(&args[2]),
                Path::new(&args[3]),
                cancellation,
            )?;
            let completed = result["cancelled_after_commit"].as_bool() != Some(true);
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(completed)
        }
        _ => Err("invalid command or arguments; use kyberia --help".into()),
    }
}

fn uses_process_cancellation(args: &[String]) -> bool {
    matches!(
        args,
        [command, _project, _request, _destination] if command == "analyze-stored-rssi" || command == "export-stored-rssi-scene"
    )
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = if uses_process_cancellation(&args) {
        match cancellation::ProcessCancellation::install() {
            Ok(cancellation) => run(&args, cancellation.token()),
            Err(error) => Err(error),
        }
    } else {
        let cancellation = NeverCancel;
        run(&args, &cancellation)
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            if error
                .downcast_ref::<stored_analysis::PublicationDurabilityError>()
                .is_some()
            {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "error": {
                            "code": "publication_durability",
                            "message": error.to_string(),
                            "committed": true,
                        }
                    })
                );
            } else if error.downcast_ref::<cancellation::Cancelled>().is_some() {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "error": {
                            "code": "cancelled",
                            "message": error.to_string(),
                            "committed": false,
                        }
                    })
                );
            } else {
                eprintln!("{}", serde_json::json!({"error": error.to_string()}));
            }
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::uses_process_cancellation;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn process_signal_registration_is_limited_to_valid_analysis_invocations() {
        assert!(uses_process_cancellation(&args(&[
            "export-stored-rssi-scene",
            "project",
            "request",
            "destination",
        ])));
        assert!(!uses_process_cancellation(&args(&[
            "export-stored-rssi-scene",
            "project",
            "request",
        ])));
        assert!(uses_process_cancellation(&args(&[
            "analyze-stored-rssi",
            "project",
            "request",
            "destination",
        ])));
        for values in [
            &[][..],
            &["--help"][..],
            &["--version"][..],
            &["new", "project", "name"][..],
            &["inspect", "project"][..],
            &["verify", "project"][..],
            &["recover-manifest", "project"][..],
            &["export-observations-parquet", "project", "destination"][..],
            &["analyze-stored-rssi", "project", "request"][..],
            &[
                "analyze-stored-rssi",
                "project",
                "request",
                "destination",
                "extra",
            ][..],
        ] {
            assert!(!uses_process_cancellation(&args(values)), "args={values:?}");
        }
    }
}
