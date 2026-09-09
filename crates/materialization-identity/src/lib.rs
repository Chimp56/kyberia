//! Stable, content-addressed identity for materialization inputs.
//!
//! This crate only identifies an already validated project baseline and an
//! already validated operation set. It does not replay operations or mutate a
//! project. Storage and materialization adapters may persist the returned
//! canonical bytes, but they must not substitute a revision counter for either
//! identity.

use kyberia_domain::{
    identity::{ContentHash, OperationId, ProjectId},
    project::{Project, ProjectError},
};
use kyberia_operation_log::{
    MAX_OPERATION_COUNT, MAX_OPERATION_WIRE_BYTES, Operation, OperationError, OperationSet,
};
use sha2::{Digest, Sha256};
use std::{fmt, io, io::Write, mem};

const BASELINE_MAGIC: &[u8] = b"KYBERIA\0PROJECT-BASELINE\0";
const OPERATION_SET_MAGIC: &[u8] = b"KYBERIA\0OPERATION-SET\0";
const IDENTITY_VERSION: u8 = 1;

/// A conservative bound on one canonical identity artifact. It bounds the
/// material held by this pure identity layer independently of the operation
/// log's per-operation and set-count limits.
pub const MAX_IDENTITY_BYTES: usize = 64 * 1024 * 1024;

const BASELINE_HEADER_BYTES: usize = BASELINE_MAGIC.len() + 1 + 16 + 8 + 8 + 8;
const MAX_PROJECT_BYTES: usize = MAX_IDENTITY_BYTES - BASELINE_HEADER_BYTES;
const OPERATION_SET_HEADER_BYTES: usize = OPERATION_SET_MAGIC.len() + 1 + 16 + 8;
const OPERATION_ENTRY_HEADER_BYTES: usize = 16 + 32 + 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    Project(ProjectError),
    Operation(OperationError),
    MalformedEncoding,
    NonCanonicalEncoding,
    HashMismatch,
    WrongProject,
    ResourceLimit(&'static str),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for IdentityError {}

impl From<ProjectError> for IdentityError {
    fn from(value: ProjectError) -> Self {
        Self::Project(value)
    }
}

impl From<OperationError> for IdentityError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaselineIdentity {
    project_id: ProjectId,
    revision: u64,
    logical_time: u64,
    project_bytes: Vec<u8>,
    canonical: Vec<u8>,
    content_hash: ContentHash,
}

impl BaselineIdentity {
    /// Canonicalize a validated domain project. The domain's serde boundary
    /// validates the aggregate on deserialization; BTreeMap fields provide
    /// deterministic ordering for the canonical JSON representation.
    pub fn from_project(project: &Project) -> Result<Self, IdentityError> {
        let project_bytes = canonical_project_bytes(project)?;
        Self::from_parts(
            project.id(),
            project.revision(),
            project.logical_time(),
            project_bytes,
        )
    }

    /// Read and verify the exact identity bytes emitted by this crate.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() > MAX_IDENTITY_BYTES {
            return Err(IdentityError::ResourceLimit("baseline_identity_bytes"));
        }
        let mut reader = Reader::new(bytes);
        reader.expect(BASELINE_MAGIC)?;
        reader.expect_byte(IDENTITY_VERSION)?;
        let project_id = ProjectId::from_bytes(reader.array_16()?)
            .map_err(|_| IdentityError::MalformedEncoding)?;
        let revision = reader.u64()?;
        let logical_time = reader.u64()?;
        let project_len = usize_from_u64(reader.u64()?, "identity_bytes")?;
        if project_len > MAX_PROJECT_BYTES {
            return Err(IdentityError::ResourceLimit("project_baseline_bytes"));
        }
        let project_len = checked_len(project_len, reader.remaining())?;
        let project_bytes = reader.take(project_len)?;
        reader.finish()?;

        let project: Project = serde_json::from_slice(project_bytes)
            .map_err(|_| IdentityError::Project(ProjectError::InvalidReceipt))?;
        let canonical_project_bytes = canonical_project_bytes(&project)?;
        if canonical_project_bytes.as_slice() != project_bytes
            || project.id() != project_id
            || project.revision() != revision
            || project.logical_time() != logical_time
        {
            return Err(IdentityError::NonCanonicalEncoding);
        }
        let identity =
            Self::from_parts(project_id, revision, logical_time, project_bytes.to_vec())?;
        if identity.canonical != bytes {
            return Err(IdentityError::NonCanonicalEncoding);
        }
        Ok(identity)
    }

    fn from_parts(
        project_id: ProjectId,
        revision: u64,
        logical_time: u64,
        project_bytes: Vec<u8>,
    ) -> Result<Self, IdentityError> {
        let total = BASELINE_HEADER_BYTES
            .checked_add(project_bytes.len())
            .ok_or(IdentityError::ResourceLimit("baseline_identity_bytes"))?;
        if total > MAX_IDENTITY_BYTES {
            return Err(IdentityError::ResourceLimit("baseline_identity_bytes"));
        }
        let canonical = encode_baseline(project_id, revision, logical_time, &project_bytes)?;
        let content_hash = digest(b"kyberia:materialization:project-baseline:v1\0", &canonical);
        Ok(Self {
            project_id,
            revision,
            logical_time,
            project_bytes,
            canonical,
            content_hash,
        })
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn logical_time(&self) -> u64 {
        self.logical_time
    }

    pub fn project_bytes(&self) -> &[u8] {
        &self.project_bytes
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    pub const fn content_hash(&self) -> ContentHash {
        self.content_hash
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationSetIdentity {
    project_id: ProjectId,
    operation_count: usize,
    canonical: Vec<u8>,
    content_hash: ContentHash,
}

impl OperationSetIdentity {
    /// Build an order-independent identity from a validated operation set.
    /// `OperationSet` exposes operations in canonical OperationId order, while
    /// each entry retains the complete canonical wire bytes and its digest.
    pub fn from_operation_set(set: &OperationSet) -> Result<Self, IdentityError> {
        let mut entries = Vec::new();
        let mut total = OPERATION_SET_HEADER_BYTES;
        for operation in set.operations() {
            let operation_bytes = operation.to_bytes()?;
            total = total
                .checked_add(OPERATION_ENTRY_HEADER_BYTES)
                .and_then(|size| size.checked_add(operation_bytes.len()))
                .ok_or(IdentityError::ResourceLimit("operation_set_identity_bytes"))?;
            if total > MAX_IDENTITY_BYTES {
                return Err(IdentityError::ResourceLimit("operation_set_identity_bytes"));
            }
            entries.push((
                operation.operation_id(),
                operation.content_hash(),
                operation_bytes,
            ));
        }
        if entries.len() > MAX_OPERATION_COUNT {
            return Err(IdentityError::ResourceLimit("operation_set_operations"));
        }
        let canonical = encode_operation_set(set.project_id(), &entries)?;
        let content_hash = digest(b"kyberia:materialization:operation-set:v1\0", &canonical);
        Ok(Self {
            project_id: set.project_id(),
            operation_count: entries.len(),
            canonical,
            content_hash,
        })
    }

    /// Decode and verify an identity, including every referenced operation and
    /// the complete operation graph. No caller-provided bytes are trusted.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() > MAX_IDENTITY_BYTES {
            return Err(IdentityError::ResourceLimit("operation_set_identity_bytes"));
        }
        let mut reader = Reader::new(bytes);
        reader.expect(OPERATION_SET_MAGIC)?;
        reader.expect_byte(IDENTITY_VERSION)?;
        let project_id = ProjectId::from_bytes(reader.array_16()?)
            .map_err(|_| IdentityError::MalformedEncoding)?;
        let count = usize_from_u64(reader.u64()?, "operation_set_operations")?;
        if count > MAX_OPERATION_COUNT {
            return Err(IdentityError::ResourceLimit("operation_set_operations"));
        }
        let mut operations = Vec::with_capacity(count.min(1024));
        let mut previous_id: Option<OperationId> = None;
        for _ in 0..count {
            let operation_id = OperationId::from_bytes(reader.array_16()?)
                .map_err(|_| IdentityError::MalformedEncoding)?;
            if previous_id.is_some_and(|previous| operation_id <= previous) {
                return Err(IdentityError::NonCanonicalEncoding);
            }
            previous_id = Some(operation_id);
            let content_hash = ContentHash::from_sha256(reader.array_32()?);
            let operation_len = usize_from_u64(reader.u64()?, "operation_wire_bytes")?;
            if operation_len > MAX_OPERATION_WIRE_BYTES {
                return Err(IdentityError::ResourceLimit("operation_wire_bytes"));
            }
            let operation_bytes = reader.take(operation_len)?;
            let operation = Operation::from_bytes(operation_bytes)?;
            if operation.project_id() != project_id {
                return Err(IdentityError::WrongProject);
            }
            if operation.operation_id() != operation_id {
                return Err(IdentityError::NonCanonicalEncoding);
            }
            if operation.content_hash() != content_hash {
                return Err(IdentityError::HashMismatch);
            }
            operations.push(operation);
        }
        reader.finish()?;

        let set = if operations.is_empty() {
            OperationSet::empty(project_id)
        } else {
            OperationSet::from_operations(operations)?
        };
        let identity = Self::from_operation_set(&set)?;
        if identity.canonical != bytes {
            return Err(IdentityError::NonCanonicalEncoding);
        }
        Ok(identity)
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn operation_count(&self) -> usize {
        self.operation_count
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    pub const fn content_hash(&self) -> ContentHash {
        self.content_hash
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializationIdentity {
    project_id: ProjectId,
    baseline: ContentHash,
    baseline_revision: u64,
    baseline_logical_time: u64,
    operation_set: ContentHash,
    operation_count: usize,
}

impl MaterializationIdentity {
    /// Bind identities from the same project. This is a reference contract,
    /// not a materializer and not a substitute for checking operation priors.
    pub fn bind(
        baseline: &BaselineIdentity,
        operation_set: &OperationSetIdentity,
    ) -> Result<Self, IdentityError> {
        if baseline.project_id() != operation_set.project_id() {
            return Err(IdentityError::WrongProject);
        }
        Ok(Self {
            project_id: baseline.project_id(),
            baseline: baseline.content_hash(),
            baseline_revision: baseline.revision(),
            baseline_logical_time: baseline.logical_time(),
            operation_set: operation_set.content_hash(),
            operation_count: operation_set.operation_count(),
        })
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn baseline_hash(&self) -> ContentHash {
        self.baseline
    }

    pub const fn baseline_revision(&self) -> u64 {
        self.baseline_revision
    }

    pub const fn baseline_logical_time(&self) -> u64 {
        self.baseline_logical_time
    }

    pub const fn operation_set_hash(&self) -> ContentHash {
        self.operation_set
    }

    pub const fn operation_count(&self) -> usize {
        self.operation_count
    }
}

fn encode_baseline(
    project_id: ProjectId,
    revision: u64,
    logical_time: u64,
    project_bytes: &[u8],
) -> Result<Vec<u8>, IdentityError> {
    let mut bytes = Vec::with_capacity(BASELINE_HEADER_BYTES + project_bytes.len());
    bytes.extend_from_slice(BASELINE_MAGIC);
    bytes.push(IDENTITY_VERSION);
    bytes.extend_from_slice(&project_id.bytes());
    bytes.extend_from_slice(&revision.to_be_bytes());
    bytes.extend_from_slice(&logical_time.to_be_bytes());
    bytes.extend_from_slice(
        &u64::try_from(project_bytes.len())
            .map_err(|_| IdentityError::ResourceLimit("project_baseline_bytes"))?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(project_bytes);
    if bytes.len() > MAX_IDENTITY_BYTES {
        return Err(IdentityError::ResourceLimit("baseline_identity_bytes"));
    }
    Ok(bytes)
}

fn canonical_project_bytes(project: &Project) -> Result<Vec<u8>, IdentityError> {
    canonical_project_bytes_with_limit(project, MAX_PROJECT_BYTES)
}

fn canonical_project_bytes_with_limit(
    project: &Project,
    limit: usize,
) -> Result<Vec<u8>, IdentityError> {
    let mut writer = BoundedWriter::new(limit);
    if serde_json::to_writer(&mut writer, project).is_err() {
        return Err(if writer.limit_exceeded() {
            IdentityError::ResourceLimit("project_baseline_bytes")
        } else {
            IdentityError::NonCanonicalEncoding
        });
    }
    Ok(writer.into_inner())
}

fn encode_operation_set(
    project_id: ProjectId,
    entries: &[(OperationId, ContentHash, Vec<u8>)],
) -> Result<Vec<u8>, IdentityError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(OPERATION_SET_MAGIC);
    bytes.push(IDENTITY_VERSION);
    bytes.extend_from_slice(&project_id.bytes());
    bytes.extend_from_slice(
        &u64::try_from(entries.len())
            .map_err(|_| IdentityError::ResourceLimit("operation_set_operations"))?
            .to_be_bytes(),
    );
    for (operation_id, content_hash, operation_bytes) in entries {
        bytes.extend_from_slice(&operation_id.bytes());
        bytes.extend_from_slice(&content_hash.bytes());
        bytes.extend_from_slice(
            &u64::try_from(operation_bytes.len())
                .map_err(|_| IdentityError::ResourceLimit("operation_wire_bytes"))?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(operation_bytes);
    }
    if bytes.len() > MAX_IDENTITY_BYTES {
        return Err(IdentityError::ResourceLimit("operation_set_identity_bytes"));
    }
    Ok(bytes)
}

fn digest(domain: &[u8], bytes: &[u8]) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    ContentHash::from_sha256(hasher.finalize().into())
}

fn checked_len(value: usize, remaining: usize) -> Result<usize, IdentityError> {
    if value > remaining {
        return Err(IdentityError::MalformedEncoding);
    }
    Ok(value)
}

fn usize_from_u64(value: u64, label: &'static str) -> Result<usize, IdentityError> {
    usize::try_from(value).map_err(|_| IdentityError::ResourceLimit(label))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    limit_exceeded: bool,
}

impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            limit_exceeded: false,
        }
    }

    fn limit_exceeded(&self) -> bool {
        self.limit_exceeded
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
            self.limit_exceeded = true;
            io::Error::other("bounded identity writer overflow")
        })?;
        if end > self.limit {
            self.limit_exceeded = true;
            return Err(io::Error::other("bounded identity writer limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: &[u8]) -> Result<(), IdentityError> {
        if self.bytes.get(self.offset..self.offset + expected.len()) != Some(expected) {
            return Err(IdentityError::MalformedEncoding);
        }
        self.offset += expected.len();
        Ok(())
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), IdentityError> {
        if self.take(1)?[0] != expected {
            return Err(IdentityError::MalformedEncoding);
        }
        Ok(())
    }

    fn array_16(&mut self) -> Result<[u8; 16], IdentityError> {
        self.take(16)?
            .try_into()
            .map_err(|_| IdentityError::MalformedEncoding)
    }

    fn array_32(&mut self) -> Result<[u8; 32], IdentityError> {
        self.take(32)?
            .try_into()
            .map_err(|_| IdentityError::MalformedEncoding)
    }

    fn u64(&mut self) -> Result<u64, IdentityError> {
        self.take(mem::size_of::<u64>())?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| IdentityError::MalformedEncoding)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], IdentityError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(IdentityError::MalformedEncoding)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(IdentityError::MalformedEncoding)?;
        self.offset = end;
        Ok(bytes)
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn finish(&self) -> Result<(), IdentityError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(IdentityError::NonCanonicalEncoding)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_domain::{
        evidence::{Evidence, UnknownReason},
        identity::{ActorDeviceId, ActorId, CalibrationId, MapAssetId, OperationId, Text},
        project::Project,
    };
    use kyberia_operation_log::{
        CausalDepth, InversePrior, LogicalTimestamp, Mutation, OperationPayload,
        OperationReference, OperationSchemaVersion, ResolutionValue,
    };

    fn project_id(value: u8) -> ProjectId {
        ProjectId::from_bytes([value; 16]).unwrap()
    }

    fn project(name: &str) -> Project {
        Project::new(project_id(1), Text::new(name).unwrap())
    }

    fn operation_id(value: u8) -> OperationId {
        OperationId::from_bytes([value; 16]).unwrap()
    }

    fn operation(value: u8, name: &str) -> Operation {
        let actor = ActorId::from_bytes([value; 16]).unwrap();
        let device = ActorDeviceId::from_bytes([value; 16]).unwrap();
        Operation::try_apply(
            operation_id(value),
            project_id(1),
            actor,
            device,
            LogicalTimestamp::new(u64::from(value)).unwrap(),
            CausalDepth::new(0),
            Vec::new(),
            Mutation::set_project_name(Text::new(name).unwrap()),
            Mutation::set_project_name(Text::new("one").unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn baseline_hash_binds_project_bytes_revision_and_logical_time() {
        let first = BaselineIdentity::from_project(&project("one")).unwrap();
        let second = BaselineIdentity::from_project(&project("two")).unwrap();
        assert_ne!(first.content_hash(), second.content_hash());
        assert_eq!(
            BaselineIdentity::from_canonical_bytes(first.canonical_bytes()).unwrap(),
            first
        );
        assert_eq!(first.project_id(), project_id(1));
        assert_eq!(first.revision(), 0);
        assert_eq!(first.logical_time(), 0);
    }

    #[test]
    fn version_one_baseline_encoding_and_hash_are_golden() {
        let identity = BaselineIdentity::from_project(&project("one")).unwrap();
        assert_eq!(
            hex(identity.canonical_bytes()),
            "4b5942455249410050524f4a4543542d424153454c494e450001010101010101010101010101010101010000000000000000000000000000000000000000000000f07b22736368656d615f76657273696f6e223a2231222c226964223a223031303130313031303130313031303130313031303130313031303130313031222c226e616d65223a226f6e65222c227265766973696f6e223a302c226c6f676963616c5f74696d65223a302c227369746573223a7b7d2c226275696c64696e6773223a7b7d2c22666c6f6f7273223a7b7d2c226d617073223a7b7d2c2263616c6962726174696f6e73223a7b7d2c226163746976655f63616c6962726174696f6e73223a7b7d2c22626f756e645f65766964656e6365223a7b7d2c226170706c6965645f6f7065726174696f6e73223a7b7d7d"
        );
        assert_eq!(
            identity.content_hash(),
            ContentHash::from_sha256([
                0x2f, 0xed, 0x9f, 0xa2, 0x09, 0x5b, 0xb9, 0x61, 0x4e, 0x51, 0xc7, 0xbb, 0x93, 0xfc,
                0x91, 0xf0, 0x32, 0xb0, 0x3f, 0x97, 0x21, 0xa4, 0x51, 0x68, 0xec, 0x01, 0xff, 0xc2,
                0x96, 0xe4, 0x79, 0x4d,
            ])
        );
    }

    #[test]
    fn baseline_rejects_project_bytes_with_unknown_or_trailing_content() {
        let identity = BaselineIdentity::from_project(&project("one")).unwrap();
        let mut bytes = identity.canonical_bytes().to_vec();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(matches!(
            BaselineIdentity::from_canonical_bytes(&bytes),
            Err(IdentityError::NonCanonicalEncoding)
                | Err(IdentityError::Project(_))
                | Err(IdentityError::MalformedEncoding)
        ));
        bytes = identity.canonical_bytes().to_vec();
        bytes.push(0);
        assert_eq!(
            BaselineIdentity::from_canonical_bytes(&bytes),
            Err(IdentityError::NonCanonicalEncoding)
        );
    }

    #[test]
    fn baseline_preserves_the_existing_v1_project_fixture() {
        let legacy = br#"{"schema_version":"1","id":"01010101010101010101010101010101","name":"one","revision":0,"logical_time":0,"sites":{},"buildings":{},"floors":{},"maps":{},"calibrations":{},"active_calibrations":{},"bound_evidence":{},"applied_operations":{}}"#;
        let project: Project = serde_json::from_slice(legacy).unwrap();
        let identity = BaselineIdentity::from_project(&project).unwrap();
        assert_eq!(identity.project_bytes(), legacy);
        assert_eq!(
            BaselineIdentity::from_canonical_bytes(identity.canonical_bytes()).unwrap(),
            identity
        );
    }

    #[test]
    fn baseline_rejects_a_header_revision_that_does_not_match_the_project() {
        let identity = BaselineIdentity::from_project(&project("one")).unwrap();
        let mut bytes = identity.canonical_bytes().to_vec();
        let revision_offset = BASELINE_MAGIC.len() + 1 + 16;
        bytes[revision_offset + 7] = 1;
        assert_eq!(
            BaselineIdentity::from_canonical_bytes(&bytes),
            Err(IdentityError::NonCanonicalEncoding)
        );
    }

    #[test]
    fn identity_decode_enforces_artifact_and_member_byte_bounds_before_parsing() {
        let mut baseline = Vec::new();
        baseline.extend_from_slice(BASELINE_MAGIC);
        baseline.push(IDENTITY_VERSION);
        baseline.extend_from_slice(&project_id(1).bytes());
        baseline.extend_from_slice(&0_u64.to_be_bytes());
        baseline.extend_from_slice(&0_u64.to_be_bytes());
        baseline.extend_from_slice(&((MAX_PROJECT_BYTES as u64) + 1).to_be_bytes());
        assert_eq!(
            BaselineIdentity::from_canonical_bytes(&baseline),
            Err(IdentityError::ResourceLimit("project_baseline_bytes"))
        );

        let mut operation_set = Vec::new();
        operation_set.extend_from_slice(OPERATION_SET_MAGIC);
        operation_set.push(IDENTITY_VERSION);
        operation_set.extend_from_slice(&project_id(1).bytes());
        operation_set.extend_from_slice(&1_u64.to_be_bytes());
        operation_set.extend_from_slice(&operation_id(1).bytes());
        operation_set.extend_from_slice(&ContentHash::from_sha256([0; 32]).bytes());
        operation_set.extend_from_slice(&((MAX_OPERATION_WIRE_BYTES as u64) + 1).to_be_bytes());
        assert_eq!(
            OperationSetIdentity::from_canonical_bytes(&operation_set),
            Err(IdentityError::ResourceLimit("operation_wire_bytes"))
        );
    }

    #[test]
    fn baseline_serialization_limit_is_a_structured_resource_error() {
        let project = project("one");
        let canonical = serde_json::to_vec(&project).unwrap();
        assert_eq!(
            canonical_project_bytes_with_limit(&project, canonical.len()).unwrap(),
            canonical
        );
        assert_eq!(
            canonical_project_bytes_with_limit(&project, canonical.len() - 1),
            Err(IdentityError::ResourceLimit("project_baseline_bytes"))
        );
    }

    #[test]
    fn operation_set_hash_is_independent_of_input_order() {
        let first = operation(1, "first");
        let second = operation(2, "second");
        let forward = OperationSet::from_operations([first.clone(), second.clone()]).unwrap();
        let reverse = OperationSet::from_operations([second, first]).unwrap();
        let forward_identity = OperationSetIdentity::from_operation_set(&forward).unwrap();
        let reverse_identity = OperationSetIdentity::from_operation_set(&reverse).unwrap();
        assert_eq!(forward_identity, reverse_identity);
        assert_eq!(
            OperationSetIdentity::from_canonical_bytes(forward_identity.canonical_bytes()).unwrap(),
            forward_identity
        );
    }

    #[test]
    fn version_one_operation_set_encoding_and_hash_are_golden() {
        let set = OperationSet::from_operations([operation(1, "first")]).unwrap();
        let identity = OperationSetIdentity::from_operation_set(&set).unwrap();
        assert_eq!(
            hex(identity.canonical_bytes()),
            "4b594245524941004f5045524154494f4e2d5345540001010101010101010101010101010101010000000000000001010101010101010101010101010101016796538666f5647f8e675353ac96c4dbb9f24b7b4347ec3158aeda7c4e86d102000000000000021a7b22736368656d615f76657273696f6e223a2231222c226f7065726174696f6e5f6964223a223031303130313031303130313031303130313031303130313031303130313031222c2270726f6a6563745f6964223a223031303130313031303130313031303130313031303130313031303130313031222c226163746f725f6964223a223031303130313031303130313031303130313031303130313031303130313031222c226465766963655f6964223a223031303130313031303130313031303130313031303130313031303130313031222c226c6f676963616c5f74696d65223a312c2263617573616c5f6465707468223a302c22706172656e7473223a5b5d2c227061796c6f6164223a7b226b696e64223a226170706c79222c2264617461223a7b226d75746174696f6e223a7b226b696e64223a227365745f70726f6a6563745f6e616d65222c2264617461223a7b226e616d65223a226669727374227d7d7d7d2c22696e7665727365223a7b226b696e64223a226170706c79222c2264617461223a7b226d75746174696f6e223a7b226b696e64223a227365745f70726f6a6563745f6e616d65222c2264617461223a7b226e616d65223a226f6e65227d7d7d7d2c22636f6e74656e745f68617368223a2236373936353338363636663536343766386536373533353361633936633464626239663234623762343334376563333135386165646137633465383664313032227d"
        );
        assert_eq!(
            identity.content_hash(),
            ContentHash::from_sha256([
                0xa7, 0x1b, 0x67, 0xc6, 0x94, 0xb0, 0x18, 0xb2, 0xab, 0x0d, 0xdf, 0x03, 0x9e, 0x2c,
                0xd2, 0xec, 0x71, 0xb2, 0xeb, 0xd2, 0x9c, 0xa8, 0xf6, 0x11, 0x8e, 0x06, 0x1f, 0xc2,
                0x88, 0xe6, 0xbf, 0x86,
            ])
        );
    }

    #[test]
    fn operation_set_hash_binds_membership_and_operation_bytes() {
        let first = operation(1, "first");
        let second = operation(2, "second");
        let changed_first = operation(1, "changed");
        let base = OperationSet::from_operations([first.clone()]).unwrap();
        let changed = OperationSet::from_operations([second]).unwrap();
        let changed_bytes = OperationSet::from_operations([changed_first]).unwrap();
        let base_identity = OperationSetIdentity::from_operation_set(&base).unwrap();
        let changed_identity = OperationSetIdentity::from_operation_set(&changed).unwrap();
        let changed_bytes_identity =
            OperationSetIdentity::from_operation_set(&changed_bytes).unwrap();
        assert_ne!(
            base_identity.content_hash(),
            changed_identity.content_hash()
        );
        assert_ne!(
            base_identity.content_hash(),
            changed_bytes_identity.content_hash()
        );

        let mut tampered = base_identity.canonical_bytes().to_vec();
        let index = tampered.len() - 1;
        tampered[index] ^= 1;
        assert!(OperationSetIdentity::from_canonical_bytes(&tampered).is_err());

        let mut tampered_hash = base_identity.canonical_bytes().to_vec();
        let hash_index = OPERATION_SET_HEADER_BYTES + 16 + 31;
        tampered_hash[hash_index] ^= 1;
        assert_eq!(
            OperationSetIdentity::from_canonical_bytes(&tampered_hash),
            Err(IdentityError::HashMismatch)
        );
    }

    #[test]
    fn empty_operation_set_has_a_project_bound_identity() {
        let set = OperationSet::empty(project_id(1));
        let identity = OperationSetIdentity::from_operation_set(&set).unwrap();
        assert_eq!(identity.operation_count(), 0);
        assert_eq!(
            OperationSetIdentity::from_canonical_bytes(identity.canonical_bytes()).unwrap(),
            identity
        );
    }

    #[test]
    fn operation_set_count_is_rejected_before_member_allocation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(OPERATION_SET_MAGIC);
        bytes.push(IDENTITY_VERSION);
        bytes.extend_from_slice(&project_id(1).bytes());
        bytes.extend_from_slice(&((MAX_OPERATION_COUNT as u64) + 1).to_be_bytes());
        assert_eq!(
            OperationSetIdentity::from_canonical_bytes(&bytes),
            Err(IdentityError::ResourceLimit("operation_set_operations"))
        );
    }

    #[test]
    fn operation_set_identity_round_trips_v2_unknown_and_resolution_values() {
        let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
        let root_calibration = CalibrationId::from_bytes([8; 16]).unwrap();
        let right_calibration = CalibrationId::from_bytes([9; 16]).unwrap();
        let root = kyberia_operation_log::Operation::try_apply_v2(
            operation_id(2),
            project_id(1),
            ActorId::from_bytes([1; 16]).unwrap(),
            ActorDeviceId::from_bytes([1; 16]).unwrap(),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, root_calibration),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            },
        )
        .unwrap();
        let left = kyberia_operation_log::Operation::try_undo_v2(
            operation_id(3),
            project_id(1),
            ActorId::from_bytes([1; 16]).unwrap(),
            ActorDeviceId::from_bytes([1; 16]).unwrap(),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![root.operation_id()],
            OperationReference::from(&root),
        )
        .unwrap();
        let right = kyberia_operation_log::Operation::try_apply_v2(
            operation_id(4),
            project_id(1),
            ActorId::from_bytes([2; 16]).unwrap(),
            ActorDeviceId::from_bytes([2; 16]).unwrap(),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![root.operation_id()],
            Mutation::activate_calibration(map_id, right_calibration),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Known(root_calibration),
            },
        )
        .unwrap();
        let resolution = kyberia_operation_log::Operation::try_resolve_v2(
            operation_id(5),
            project_id(1),
            ActorId::from_bytes([3; 16]).unwrap(),
            ActorDeviceId::from_bytes([3; 16]).unwrap(),
            LogicalTimestamp::new(3).unwrap(),
            CausalDepth::new(2),
            vec![left.operation_id(), right.operation_id()],
            OperationReference::from(&left),
            OperationReference::from(&right),
            ResolutionValue::activate_calibration(
                map_id,
                Evidence::Unknown(UnknownReason::NotMeasured),
            ),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Known(root_calibration),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([root, left, right, resolution]).unwrap();
        let identity = OperationSetIdentity::from_operation_set(&set).unwrap();
        let decoded =
            OperationSetIdentity::from_canonical_bytes(identity.canonical_bytes()).unwrap();
        assert_eq!(decoded, identity);
        assert_eq!(decoded.operation_count(), 4);

        let mut reader = Reader::new(decoded.canonical_bytes());
        reader.expect(OPERATION_SET_MAGIC).unwrap();
        reader.expect_byte(IDENTITY_VERSION).unwrap();
        assert_eq!(reader.array_16().unwrap(), project_id(1).bytes());
        assert_eq!(reader.u64().unwrap(), 4);
        let mut saw_unknown_prior = false;
        let mut saw_unknown_resolution = false;
        for _ in 0..4 {
            reader.array_16().unwrap();
            reader.array_32().unwrap();
            let length = reader.u64().unwrap() as usize;
            let operation = Operation::from_bytes(reader.take(length).unwrap()).unwrap();
            assert_eq!(operation.schema_version(), OperationSchemaVersion::V2);
            match operation.payload() {
                OperationPayload::Apply { .. }
                    if matches!(
                        operation.inverse(),
                        kyberia_operation_log::InverseMetadata::ApplyV2 {
                            prior: InversePrior::MapCalibration {
                                calibration: Evidence::Unknown(UnknownReason::NotMeasured),
                                ..
                            }
                        }
                    ) =>
                {
                    saw_unknown_prior = true
                }
                OperationPayload::ResolveV2 {
                    value:
                        ResolutionValue::Calibration {
                            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
                            ..
                        },
                    ..
                } => saw_unknown_resolution = true,
                _ => {}
            }
        }
        reader.finish().unwrap();
        assert!(saw_unknown_prior);
        assert!(saw_unknown_resolution);
    }

    #[test]
    fn operation_set_identity_rejects_tampered_v2_operation_bytes_and_hash() {
        let operation = kyberia_operation_log::Operation::try_apply_v2(
            operation_id(9),
            project_id(1),
            ActorId::from_bytes([9; 16]).unwrap(),
            ActorDeviceId::from_bytes([9; 16]).unwrap(),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(
                MapAssetId::from_bytes([7; 16]).unwrap(),
                CalibrationId::from_bytes([8; 16]).unwrap(),
            ),
            InversePrior::MapCalibration {
                map_id: MapAssetId::from_bytes([7; 16]).unwrap(),
                calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([operation]).unwrap();
        let identity = OperationSetIdentity::from_operation_set(&set).unwrap();

        let mut tampered_bytes = identity.canonical_bytes().to_vec();
        *tampered_bytes.last_mut().unwrap() ^= 1;
        assert!(OperationSetIdentity::from_canonical_bytes(&tampered_bytes).is_err());

        let mut tampered_hash = identity.canonical_bytes().to_vec();
        let hash_index = OPERATION_SET_HEADER_BYTES + 16 + 31;
        tampered_hash[hash_index] ^= 1;
        assert_eq!(
            OperationSetIdentity::from_canonical_bytes(&tampered_hash),
            Err(IdentityError::HashMismatch)
        );
    }

    #[test]
    fn binding_rejects_project_mismatch_and_keeps_counters_distinct() {
        let baseline = BaselineIdentity::from_project(&project("one")).unwrap();
        let other_set = OperationSet::empty(project_id(2));
        let other_identity = OperationSetIdentity::from_operation_set(&other_set).unwrap();
        assert_eq!(
            MaterializationIdentity::bind(&baseline, &other_identity),
            Err(IdentityError::WrongProject)
        );

        let set = OperationSet::from_operations([operation(1, "first")]).unwrap();
        let set_identity = OperationSetIdentity::from_operation_set(&set).unwrap();
        let binding = MaterializationIdentity::bind(&baseline, &set_identity).unwrap();
        assert_eq!(binding.baseline_revision(), baseline.revision());
        assert_eq!(binding.baseline_logical_time(), baseline.logical_time());
        assert_ne!(
            binding.baseline_revision(),
            set_identity.operation_count() as u64
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
