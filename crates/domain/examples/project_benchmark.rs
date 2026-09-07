//! Reproducible input sizes, descriptive timings only; no machine-independent
//! speed threshold is asserted. Run in release mode on an otherwise idle host.
use kyberia_domain::{evidence::*, identity::*, project::*};
use std::num::NonZeroU64;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for count in [100_u64, 1_000, 10_000] {
        let mut state = Project::new(ProjectId::from_bytes([1; 16])?, Text::new("Benchmark")?);
        let start = Instant::now();
        for i in 1..=count {
            let mut bytes = [0; 16];
            bytes[8..].copy_from_slice(&i.to_be_bytes());
            let request = CommandRequest {
                schema_version: SchemaVersion::V1,
                operation_id: OperationId::from_bytes(bytes)?,
                project_id: state.id(),
                actor_id: ActorId::from_bytes([1; 16])?,
                device_id: ActorDeviceId::from_bytes([1; 16])?,
                logical_time: NonZeroU64::new(i).ok_or("zero logical time")?,
                expected_revision: state.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::CreateSite(Site {
                    id: SiteId::from_bytes(bytes)?,
                    name: Text::new("Site")?,
                }),
            };
            state = state.execute(request)?.project;
        }
        println!(
            "sites={count} operations={count} elapsed_ms={:.3} final_revision={}",
            start.elapsed().as_secs_f64() * 1000.0,
            state.revision()
        );
    }
    Ok(())
}
