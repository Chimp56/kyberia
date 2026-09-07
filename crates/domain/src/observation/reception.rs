//! Source API response timing is evidence about retrieval, never RF capture.
use super::ObservationEnvelope;
use crate::{ValidationError, evidence::Evidence, time::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ResponseWire", into = "ResponseWire")]
pub struct SourceResponseTiming {
    returned_at: CaptureTime,
    api_window: Evidence<MonotonicWindow>,
}
impl SourceResponseTiming {
    pub fn new(
        returned_at: CaptureTime,
        api_window: Evidence<MonotonicWindow>,
    ) -> Result<Self, ValidationError> {
        if let Evidence::Known(model) = &returned_at.synchronization {
            if let Evidence::Known(time) = &returned_at.monotonic
                && model.epoch != time.epoch
            {
                return Err(ValidationError::ClockEpochMismatch);
            }
            if let Evidence::Known(window) = &api_window
                && model.epoch != window.start().epoch
            {
                return Err(ValidationError::ClockEpochMismatch);
            }
        }
        if let (Evidence::Known(time), Evidence::Known(window)) =
            (&returned_at.monotonic, &api_window)
        {
            time.elapsed_since(window.end())?;
        }
        Ok(Self {
            returned_at,
            api_window,
        })
    }
    pub const fn returned_at(&self) -> &CaptureTime {
        &self.returned_at
    }
    pub const fn api_window(&self) -> &Evidence<MonotonicWindow> {
        &self.api_window
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseWire {
    returned_at: CaptureTime,
    api_window: Evidence<MonotonicWindow>,
}
impl TryFrom<ResponseWire> for SourceResponseTiming {
    type Error = ValidationError;
    fn try_from(w: ResponseWire) -> Result<Self, Self::Error> {
        Self::new(w.returned_at, w.api_window)
    }
}
impl From<SourceResponseTiming> for ResponseWire {
    fn from(t: SourceResponseTiming) -> Self {
        Self {
            returned_at: t.returned_at,
            api_window: t.api_window,
        }
    }
}

/// Independent transport receipt version; the nested observation keeps V2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceptionSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ReceptionWire", into = "ReceptionWire")]
pub struct ReceivedObservation {
    envelope: ObservationEnvelope,
    source_response: Evidence<SourceResponseTiming>,
}
impl ReceivedObservation {
    pub fn new(
        envelope: ObservationEnvelope,
        source_response: Evidence<SourceResponseTiming>,
    ) -> Result<Self, ValidationError> {
        if let Evidence::Known(response) = &source_response
            && let (Evidence::Known(captured), Evidence::Known(returned)) = (
                &envelope.data().time.monotonic,
                &response.returned_at.monotonic,
            )
            && captured.epoch == returned.epoch
        {
            // Hardware capture clocks may differ from the source API clock.
            // Only an explicitly identical epoch permits this comparison.
            returned.elapsed_since(*captured)?;
        }
        Ok(Self {
            envelope,
            source_response,
        })
    }
    pub const fn envelope(&self) -> &ObservationEnvelope {
        &self.envelope
    }
    pub const fn source_response(&self) -> &Evidence<SourceResponseTiming> {
        &self.source_response
    }
    pub fn into_parts(self) -> (ObservationEnvelope, Evidence<SourceResponseTiming>) {
        (self.envelope, self.source_response)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceptionWire {
    schema_version: ReceptionSchemaVersion,
    envelope: ObservationEnvelope,
    source_response: Evidence<SourceResponseTiming>,
}
impl TryFrom<ReceptionWire> for ReceivedObservation {
    type Error = ValidationError;
    fn try_from(w: ReceptionWire) -> Result<Self, Self::Error> {
        Self::new(w.envelope, w.source_response)
    }
}
impl From<ReceivedObservation> for ReceptionWire {
    fn from(r: ReceivedObservation) -> Self {
        Self {
            schema_version: ReceptionSchemaVersion::V1,
            envelope: r.envelope,
            source_response: r.source_response,
        }
    }
}
