use super::*;

/// Registry-backed identities supplied by the caller, scoped by exact process/interface key.
/// These identify the receiving collector; they never identify the transmitting AP.
#[derive(Clone, Debug)]
pub struct SourceMapping {
    pub source_id: SourceId,
    pub sensor_id: Evidence<SensorId>,
    pub adapter_id: Evidence<AdapterId>,
}
/// Optional transmitter correlation is scoped to an observation with known BSSID.
/// The caller must supply auditable identity evidence, not an interface-derived radio.
#[derive(Clone, Debug)]
pub struct ObservationMapping {
    pub observation_id: ObservationId,
    pub transmitter_radio: Evidence<RadioId>,
    pub transmitter_bss: Evidence<BssId>,
    pub identity_evidence: Evidence<ArtifactReference>,
}
/// Explicit application identity and privacy decisions. This adapter creates no IDs.
#[derive(Clone, Debug)]
pub struct MappingContext {
    pub expected_process_session: String,
    pub session_id: SessionId,
    pub collector_id: CollectorId,
    pub clock_epoch: ClockEpochId,
    pub sources: BTreeMap<String, SourceMapping>,
    pub observations: BTreeMap<String, ObservationMapping>,
    pub privacy: PrivacyState,
}
#[derive(Clone, Debug, Serialize)]
pub struct Completion {
    pub status: TerminalStatus,
    pub reason: Text,
    pub partial: bool,
    pub observation_count: u16,
}
/// Canonical observations accompanied by a terminal result, never implied survey admission.
#[derive(Clone, Debug, Serialize)]
pub struct SourceRecord {
    pub reference: ArtifactReference,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Serialize)]
pub struct NormalizedCapture {
    /// Fixture origin remains explicit even for an empty probe or error stream.
    pub evidence_origin: SourceKind,
    /// Exact bounded input, including hello, capabilities and terminal evidence.
    /// Contains identifiers if operator explicitly enabled them. Apply project policy before storage.
    pub source_records: Vec<SourceRecord>,
    pub schema_version: SchemaVersion,
    pub collector_build: ContentHash,
    pub capabilities: Evidence<CapabilityDocument>,
    pub observations: Vec<ReceivedObservation>,
    pub completion: Completion,
}
fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}
fn reason(r: Reason) -> UnknownReason {
    match r {
        Reason::SourceDidNotProvide | Reason::InvalidOrUnavailableSourceValue => {
            UnknownReason::SourceDidNotProvide
        }
        Reason::NotCalibrated | Reason::NotCollected => UnknownReason::NotMeasured,
        Reason::UnsupportedSourceEnum
        | Reason::NotSupportedByCollector
        | Reason::NotImplementedByCollector => UnknownReason::UnsupportedCapability,
        Reason::NotRetainedByCollector => UnknownReason::NotRetained,
        Reason::Redacted => UnknownReason::Redacted,
    }
}
fn evidence<T, U>(e: &E<T>, f: impl FnOnce(&T) -> Result<U>) -> Result<Evidence<U>> {
    match e {
        E::Known { value } => Ok(Evidence::Known(f(value)?)),
        E::Unknown { reason: r } => Ok(Evidence::Unknown(reason(*r))),
    }
}
fn capture_unknown() -> CaptureTime {
    CaptureTime {
        wall: unknown(),
        monotonic: unknown(),
        synchronization: unknown(),
    }
}
fn receipt_time(r: &Receipt, epoch: ClockEpochId) -> Result<CaptureTime> {
    Ok(CaptureTime {
        wall: Evidence::Known(WallClockReading {
            time: r.utc,
            source: text("CoreWLAN collector Foundation.Date result receipt")?,
            precision: r.precision,
            uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
        }),
        monotonic: Evidence::Known(MonotonicTimestamp {
            epoch,
            nanoseconds: r.nanos,
        }),
        synchronization: Evidence::Unknown(UnknownReason::ClockUnavailable),
    })
}
fn canonical_channel(c: &E<Channel>) -> Result<Evidence<ChannelContext>> {
    evidence(c, |c| {
        Ok(ChannelContext {
            band: evidence(&c.band, |b| match b.as_str() {
                "2.4_ghz" => Ok(Band::Ghz2_4),
                "5_ghz" => Ok(Band::Ghz5),
                "6_ghz" => Ok(Band::Ghz6),
                _ => Err(Error::new(K::InvalidField("band"))),
            })?,
            // CoreWLAN's channelNumber lacks primary/center geometry guarantees here.
            primary_channel: unknown(),
            primary_frequency: unknown(),
            center_frequency: unknown(),
            second_center_frequency: unknown(),
            width: evidence(&c.width_mhz, |v| {
                Megahertz::new(f64::from(*v)).map_err(|_| Error::new(K::Canonical))
            })?,
            puncturing: unknown(),
        })
    })
}
fn capability_document(
    c: &Capabilities,
    r: &Receipt,
    h: &Hello,
    ctx: &MappingContext,
) -> Result<CapabilityDocument> {
    let mut entries = BTreeMap::new();
    let evidence_text =
        text("macOS collector v1 capability probe; authorization and interface state may change")?;
    let nearby = if c.nearby_scan.state == "available" {
        CapabilityState::Available {
            evidence: evidence_text.clone(),
        }
    } else {
        CapabilityState::Unavailable {
            reason: if !c.location_services_enabled
                || c.location_authorization != AuthorizationState::Authorized
            {
                UnknownReason::PermissionDenied
            } else {
                UnknownReason::NotObservable
            },
            remediation: text(&c.nearby_scan.condition)?,
        }
    };
    entries.insert(Capability::NearbyScan, nearby);
    entries.insert(
        Capability::NoiseDbm,
        CapabilityState::Conditional {
            condition: text(&c.noise_dbm.condition)?,
            evidence: evidence_text.clone(),
        },
    );
    for cap in [
        Capability::MonitorFrames,
        Capability::ChannelControl,
        Capability::ChannelHopping,
        Capability::Radiotap,
        Capability::PerChainSignal,
        Capability::Fcs,
        Capability::RetryFlag,
        Capability::PhyMetadata,
        Capability::ConcurrentManagedMonitor,
        Capability::SpectrumSweep,
        Capability::Gps,
        Capability::Pose,
        Capability::CurrentLink,
        Capability::ActiveProbes,
    ] {
        entries.insert(
            cap,
            CapabilityState::Unavailable {
                reason: UnknownReason::UnsupportedCapability,
                remediation: text(
                    "Not implemented by this CoreWLAN collector; select a capable adapter",
                )?,
            },
        );
    }
    for (band, cap) in [
        (1, Capability::Band2Ghz),
        (2, Capability::Band5Ghz),
        (3, Capability::Band6Ghz),
    ] {
        if c.sources
            .iter()
            .any(|s| s.reported_band_enums.contains(&band))
        {
            entries.insert(cap,CapabilityState::Conditional {condition:text("API-reported band support; actual permitted scans and per-source availability require validation")?,evidence:evidence_text.clone()});
        }
    }
    Ok(CapabilityDocument {
        schema_version: SchemaVersion::V1,
        collector_id: ctx.collector_id,
        collector_version: text(&h.collector_version)?,
        probed_at: receipt_time(r, ctx.clock_epoch)?,
        entries,
        raw_payload_policy: RawPayloadPolicy::Discard,
    })
}
/// No observations are returned if any mapping, privacy or canonical conversion fails.
/// Known raw references hash exact NDJSON record bytes; retaining/resolving those bytes
/// is a separate storage/privacy decision and is not performed by this pure function.
pub fn normalize(stream: &DecodedStream, ctx: &MappingContext) -> Result<NormalizedCapture> {
    check(
        ctx.expected_process_session == stream.session,
        K::MissingMapping("process session"),
    )?;
    let Body::Hello(h) = &stream.records[0].body else {
        return Err(Error::new(K::Sequence));
    };
    check(
        matches!(
            ctx.privacy.payload,
            PayloadRetention::Discarded | PayloadRetention::NotApplicable
        ),
        K::Privacy,
    )?;
    if h.identifier_policy == Policy::Redacted {
        check(
            matches!(ctx.privacy.identifiers, IdentifierPolicy::Redacted),
            K::Privacy,
        )?;
    } else {
        check(
            matches!(
                ctx.privacy.identifiers,
                IdentifierPolicy::OwnedInfrastructure | IdentifierPolicy::ExplicitResearchConsent
            ),
            K::Privacy,
        )?;
    }
    let mut canonical_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    for key in stream.source_keys() {
        let m = ctx
            .sources
            .get(key)
            .ok_or_else(|| Error::new(K::MissingMapping("source")))?;
        check(
            source_ids.insert(m.source_id),
            K::MissingMapping("unique source"),
        )?;
    }
    let partial = match &stream
        .records
        .last()
        .ok_or_else(|| Error::new(K::Incomplete))?
        .body
    {
        Body::Complete(c) => c.partial,
        _ => return Err(Error::new(K::Incomplete)),
    };
    let mut observations = Vec::new();
    let mut capabilities = unknown();
    let mut completion = None;
    for (index, r) in stream.records.iter().enumerate() {
        (|| {
            match &r.body {
                Body::Capabilities(c) => {
                    capabilities = Evidence::Known(capability_document(c, &r.time, h, ctx)?)
                }
                Body::ScanObservation(o) => {
                    let m = ctx
                        .sources
                        .get(&o.source.source_id)
                        .ok_or_else(|| Error::new(K::MissingMapping("source")))?;
                    let mapping = ctx
                        .observations
                        .get(&o.observation_id)
                        .ok_or_else(|| Error::new(K::MissingMapping("observation")))?;
                    let id = mapping.observation_id;
                    check(
                        (!matches!(mapping.transmitter_radio, Evidence::Known(_))
                            && !matches!(mapping.transmitter_bss, Evidence::Known(_)))
                            || (known(&o.bssid).is_some()
                                && matches!(mapping.identity_evidence, Evidence::Known(_))),
                        K::MissingMapping("transmitter assignment evidence"),
                    )?;
                    check(
                        canonical_ids.insert(id),
                        K::MissingMapping("unique observation"),
                    )?;
                    let mut quality = vec![QualityFlag::ClockUncertain];
                    if h.evidence_origin == Origin::SyntheticFixture {
                        quality.push(QualityFlag::SyntheticFixture);
                    }
                    if partial {
                        quality.push(QualityFlag::PartialCapture);
                    }
                    let signal = SignalReading {
                        rssi_dbm: evidence(&o.rssi_dbm, |v| {
                            Dbm::new(f64::from(*v)).map_err(|_| Error::new(K::Canonical))
                        })?,
                        noise_dbm: evidence(&o.noise_dbm, |v| {
                            Dbm::new(f64::from(*v)).map_err(|_| Error::new(K::Canonical))
                        })?,
                        chains: Vec::new(),
                        calibration: Evidence::Known(CalibrationState::Uncalibrated),
                        measurement_method: text(&o.measurement_method)?,
                    };
                    let source = SourceDescriptor {
                        source_id: m.source_id,
                        collector_id: ctx.collector_id,
                        sensor_id: m.sensor_id.clone(),
                        adapter_id: m.adapter_id.clone(),
                        kind: if h.evidence_origin == Origin::SyntheticFixture {
                            SourceKind::SyntheticFixture
                        } else {
                            SourceKind::NativeApi
                        },
                        source_name: text(&o.source.source_api)?,
                        source_version: if o.source.framework_version == "unknown" {
                            unknown()
                        } else {
                            Evidence::Known(text(&o.source.framework_version)?)
                        },
                        source_schema_version: text(PROTOCOL)?,
                        adapter_name: text(&h.collector)?,
                        adapter_version: text(&h.collector_version)?,
                        parser_version: text(concat!(
                            "kyberia-capture-adapter/",
                            env!("CARGO_PKG_VERSION")
                        ))?,
                        driver_version: unknown(),
                        os_version: Evidence::Known(text(&h.os_version)?),
                    };
                    let envelope = ObservationEnvelope::new(EnvelopeData {
                        schema_version: ObservationSchemaVersion::V2,
                        id,
                        session_id: ctx.session_id,
                        source,
                        time: capture_unknown(),
                        pose: Evidence::Unknown(UnknownReason::NotMeasured),
                        channel: canonical_channel(&o.channel)?,
                        dwell: unknown(),
                        privacy: ctx.privacy.clone(),
                        quality,
                        raw_source: Evidence::Known(r.raw.clone()),
                        payload: ObservationPayload::Scan(ScanObservation {
                            identity: RadioIdentityEvidence {
                                physical_device: unknown(),
                                radio: mapping.transmitter_radio.clone(),
                                bss: mapping.transmitter_bss.clone(),
                                bssid: evidence(&o.bssid, |b| mac(b))?,
                                ess: unknown(),
                                mld: unknown(),
                                link_id: unknown(),
                                client: Evidence::Unknown(UnknownReason::NotApplicable),
                                grouping_evidence: mapping.identity_evidence.clone(),
                            },
                            ssid: evidence(&o.ssid_octets_base64, |v| ssid(v))?,
                            signal,
                            information_elements: Evidence::Unknown(UnknownReason::NotRetained),
                            result_age: unknown(),
                        }),
                    })
                    .map_err(|_| Error::new(K::Canonical))?;
                    let api_window = MonotonicWindow::new(
                        MonotonicTimestamp {
                            epoch: ctx.clock_epoch,
                            nanoseconds: decimal(&o.api_window.start_monotonic_ns)?,
                        },
                        MonotonicTimestamp {
                            epoch: ctx.clock_epoch,
                            nanoseconds: decimal(&o.api_window.end_monotonic_ns)?,
                        },
                    )
                    .map_err(|_| Error::new(K::Canonical))?;
                    let response = SourceResponseTiming::new(
                        receipt_time(&r.time, ctx.clock_epoch)?,
                        Evidence::Known(api_window),
                    )
                    .map_err(|_| Error::new(K::Canonical))?;
                    observations.push(
                        ReceivedObservation::new(envelope, Evidence::Known(response))
                            .map_err(|_| Error::new(K::Canonical))?,
                    );
                }
                Body::Complete(c) => {
                    completion = Some(Completion {
                        status: c.status,
                        reason: text(&c.reason)?,
                        partial: c.partial,
                        observation_count: c.observation_count,
                    })
                }
                _ => {}
            }
            Ok(())
        })()
        .map_err(|mut e: Error| {
            e.record = Some(index);
            e
        })?;
    }
    Ok(NormalizedCapture {
        evidence_origin: if h.evidence_origin == Origin::SyntheticFixture {
            SourceKind::SyntheticFixture
        } else {
            SourceKind::NativeApi
        },
        source_records: stream
            .records
            .iter()
            .map(|r| SourceRecord {
                reference: r.raw.clone(),
                bytes: r.bytes.clone(),
            })
            .collect(),
        schema_version: SchemaVersion::V1,
        collector_build: ContentHash::try_from(h.collector_build[7..].to_string())
            .map_err(|_| Error::new(K::Provenance))?,
        capabilities,
        observations,
        completion: completion.ok_or_else(|| Error::new(K::Incomplete))?,
    })
}
