//! Independent, bounded IEEE 802.11 management-frame normalization.
//!
//! Parsed evidence is immutable outside this crate. Consumers use accessors;
//! safe Rust cannot construct or mutate normalized frames:
//! ```compile_fail
//! # use kyberia_ieee80211::{parse, InputFraming, ManagementSubtype};
//! # let raw = [0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
//! let mut frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
//! frame.subtype = ManagementSubtype::Beacon;
//! ```
//! ```compile_fail
//! # use kyberia_ieee80211::{parse, InputFraming};
//! # let raw = [0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
//! let mut frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
//! frame.elements.clear();
//! ```
//! ```compile_fail
//! # use kyberia_ieee80211::ManagementFrame;
//! let _ = ManagementFrame { schema_version: 1 };
//! ```
#![forbid(unsafe_code)]

const HEADER_LEN: usize = 24;
const RESPONSE_FIXED_LEN: usize = 12;
const CANONICAL_MAGIC: &[u8; 7] = b"KY11IE\0";
const CANONICAL_VERSION: u16 = 1;

/// Default limits accept the maximum 802.11 MPDU while bounding all retained
/// structures and the deterministic amount of parsing work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_frame_bytes: usize,
    pub max_elements: usize,
    pub max_ie_payload_bytes: usize,
    pub max_work_units: usize,
    pub max_canonical_bytes: usize,
    /// Cumulative logical bytes allocated for retained, derived, scratch, or
    /// output buffers during one public operation.
    pub max_allocation_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 11_454,
            max_elements: 1_024,
            max_ie_payload_bytes: 11_418,
            max_work_units: 128_000,
            max_canonical_bytes: 11_468,
            max_allocation_bytes: 256 * 1_024,
        }
    }
}

pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFraming {
    /// Input ends with the final IE payload byte.
    FcsAbsent,
    /// Input ends with a four-byte little-endian IEEE CRC-32, which is checked.
    FcsPresentAndValidate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementSubtype {
    ProbeRequest,
    ProbeResponse,
    Beacon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacAddress([u8; 6]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressRoles {
    /// Address 1: destination/receiver for all supported management frames.
    receiver: MacAddress,
    /// Address 2: source/transmitter for all supported management frames.
    transmitter: MacAddress,
    /// Address 3 is the BSSID field. In a Probe Request it can be wildcard and
    /// is not inferred to identify the transmitter.
    bssid_field: MacAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseFixedFields {
    timestamp: u64,
    beacon_interval_tu: u16,
    capability_information: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SsidValue {
    Hidden,
    Wildcard,
    Binary(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodedRate {
    raw: u8,
    basic: bool,
    /// The encoded rate magnitude in units of 500 kbit/s. This is not an
    /// expected or negotiated PHY rate.
    units_500_kbps: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimStructure {
    dtim_count: u8,
    dtim_period: u8,
    bitmap_control: u8,
    partial_virtual_bitmap: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountryStructure {
    country_environment: [u8; 3],
    triplets: Vec<[u8; 3]>,
    padding: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementProblem {
    SsidTooLong,
    RatesLength,
    DsLength,
    TimContext,
    TimTooShort,
    TimTooLong,
    TimInvalidPeriod,
    TimCountExceedsPeriod,
    CountryTooShort,
    CountryInvalidPadding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementDecode {
    Ssid(SsidValue),
    SupportedRates(Vec<EncodedRate>),
    ExtendedSupportedRates(Vec<EncodedRate>),
    DsParameterChannel(u8),
    Tim(TimStructure),
    Country(CountryStructure),
    Extension { extension_id: u8, body: Vec<u8> },
    Unknown,
    Malformed(ElementProblem),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InformationElement {
    id: u8,
    extension_id: Option<u8>,
    /// Byte offset of the IE identifier in the MAC MPDU (never radiotap).
    offset: u32,
    /// Byte offset of the first payload byte in the MAC MPDU.
    payload_offset: u32,
    payload: Vec<u8>,
    decoded: ElementDecode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementKey {
    id: u8,
    extension_id: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeatedElement {
    key: ElementKey,
    indices: Vec<u32>,
    payloads_identical: bool,
    /// True only for foundational elements whose contract is singleton.
    violates_singleton_cardinality: bool,
    /// True only when a singleton has non-identical raw payloads.
    contradictory: bool,
}

/// Numeric standards identity for an information element. This deliberately
/// carries no clause number, citation, or registry display name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardsElementReference {
    Ieee80211ElementIdentifier { id: u8, extension_id: Option<u8> },
}

/// Zero-copy view over one successfully parsed management frame for an IE
/// explorer. Elements remain in on-air order and retain duplicates.
#[derive(Clone, Copy, Debug)]
pub struct InformationElementExplorer<'a> {
    frame: &'a ManagementFrame,
}

#[derive(Clone, Copy, Debug)]
pub struct ExplorerElement<'a> {
    frame: &'a ManagementFrame,
    index: usize,
}

pub struct ExplorerElementIter<'a> {
    frame: &'a ManagementFrame,
    next_index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ElementWarnings<'a> {
    malformed: Option<ElementProblem>,
    repetition: Option<&'a RepeatedElement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Copy, Debug)]
pub struct ElementChange<'before, 'after> {
    kind: ElementChangeKind,
    occurrence: usize,
    before: Option<ExplorerElement<'before>>,
    after: Option<ExplorerElement<'after>>,
}

/// Deterministic IE-content diff. Changes are ordered by numeric IE identity
/// and occurrence within that identity; each explorer view separately retains
/// original frame order, and `wire_ie_sequence_changed` reports any ordered
/// raw IE sequence difference.
#[derive(Debug)]
pub struct InformationElementDiff<'before, 'after> {
    changes: Vec<ElementChange<'before, 'after>>,
    subtype_changed: bool,
    wire_ie_sequence_changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ElementOccurrence {
    key: ElementKey,
    occurrence: usize,
    index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagementFrame {
    schema_version: u16,
    framing: InputFraming,
    raw_mpdu: Vec<u8>,
    subtype: ManagementSubtype,
    frame_control: u16,
    duration_id: u16,
    addresses: AddressRoles,
    sequence_number: u16,
    fragment_number: u8,
    fixed: Option<ResponseFixedFields>,
    elements: Vec<InformationElement>,
    repeated_elements: Vec<RepeatedElement>,
}

/// Resource consumption for one completed public operation. Allocation bytes
/// are conservative logical bytes, including retained duplicate views and
/// temporary grouping/output buffers, rather than allocator-specific capacity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceUsage {
    work_units: usize,
    allocation_bytes: usize,
}

impl MacAddress {
    pub fn octets(self) -> [u8; 6] {
        self.0
    }
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}
impl AddressRoles {
    pub fn receiver(&self) -> MacAddress {
        self.receiver
    }
    pub fn transmitter(&self) -> MacAddress {
        self.transmitter
    }
    pub fn bssid_field(&self) -> MacAddress {
        self.bssid_field
    }
}
impl ResponseFixedFields {
    pub fn timestamp(self) -> u64 {
        self.timestamp
    }
    pub fn beacon_interval_tu(self) -> u16 {
        self.beacon_interval_tu
    }
    pub fn capability_information(self) -> u16 {
        self.capability_information
    }
}
impl EncodedRate {
    pub fn raw(self) -> u8 {
        self.raw
    }
    pub fn basic(self) -> bool {
        self.basic
    }
    pub fn units_500_kbps(self) -> u8 {
        self.units_500_kbps
    }
}
impl TimStructure {
    pub fn dtim_count(&self) -> u8 {
        self.dtim_count
    }
    pub fn dtim_period(&self) -> u8 {
        self.dtim_period
    }
    pub fn bitmap_control(&self) -> u8 {
        self.bitmap_control
    }
    pub fn partial_virtual_bitmap(&self) -> &[u8] {
        &self.partial_virtual_bitmap
    }
}
impl CountryStructure {
    pub fn country_environment(&self) -> [u8; 3] {
        self.country_environment
    }
    pub fn triplets(&self) -> &[[u8; 3]] {
        &self.triplets
    }
    pub fn padding(&self) -> Option<u8> {
        self.padding
    }
}
impl InformationElement {
    pub fn id(&self) -> u8 {
        self.id
    }
    pub fn extension_id(&self) -> Option<u8> {
        self.extension_id
    }
    pub fn offset(&self) -> u32 {
        self.offset
    }
    pub fn payload_offset(&self) -> u32 {
        self.payload_offset
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn decoded(&self) -> &ElementDecode {
        &self.decoded
    }
}
impl ElementKey {
    pub fn id(self) -> u8 {
        self.id
    }
    pub fn extension_id(self) -> Option<u8> {
        self.extension_id
    }
}
impl RepeatedElement {
    pub fn key(&self) -> ElementKey {
        self.key
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    pub fn payloads_identical(&self) -> bool {
        self.payloads_identical
    }
    pub fn violates_singleton_cardinality(&self) -> bool {
        self.violates_singleton_cardinality
    }
    pub fn contradictory(&self) -> bool {
        self.contradictory
    }
}

impl<'a> InformationElementExplorer<'a> {
    pub fn subtype(&self) -> ManagementSubtype {
        self.frame.subtype
    }

    /// Original management MPDU bytes, including a validated FCS when the
    /// caller supplied one.
    pub fn raw_mpdu(&self) -> &'a [u8] {
        self.frame.raw_mpdu()
    }

    pub fn elements(&self) -> ExplorerElementIter<'a> {
        ExplorerElementIter {
            frame: self.frame,
            next_index: 0,
        }
    }
}

impl<'a> Iterator for ExplorerElementIter<'a> {
    type Item = ExplorerElement<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index >= self.frame.elements.len() {
            return None;
        }
        let index = self.next_index;
        self.next_index += 1;
        Some(ExplorerElement {
            frame: self.frame,
            index,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.frame.elements.len() - self.next_index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ExplorerElementIter<'_> {}

impl<'a> ExplorerElement<'a> {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn standards_reference(&self) -> StandardsElementReference {
        let element = &self.frame.elements[self.index];
        StandardsElementReference::Ieee80211ElementIdentifier {
            id: element.id,
            extension_id: element.extension_id,
        }
    }

    /// The complete raw TLV, including the identifier and length octets.
    pub fn raw_bytes(&self) -> &[u8] {
        let element = &self.frame.elements[self.index];
        let start = element.offset as usize;
        let end = element.payload_offset as usize + element.payload.len();
        &self.frame.raw_mpdu[start..end]
    }

    pub fn raw_payload(&self) -> &[u8] {
        self.frame.elements[self.index].payload()
    }

    pub fn decoded(&self) -> &ElementDecode {
        self.frame.elements[self.index].decoded()
    }

    pub fn is_unrecognized_by_decoder(&self) -> bool {
        matches!(self.decoded(), ElementDecode::Unknown)
    }

    pub fn warnings(&self) -> ElementWarnings<'a> {
        let element = &self.frame.elements[self.index];
        let malformed = match &element.decoded {
            ElementDecode::Malformed(problem) => Some(*problem),
            _ => None,
        };
        let key = ElementKey {
            id: element.id,
            extension_id: element.extension_id,
        };
        let repetition = self
            .frame
            .repeated_elements
            .binary_search_by_key(&key, RepeatedElement::key)
            .ok()
            .map(|index| &self.frame.repeated_elements[index]);
        ElementWarnings {
            malformed,
            repetition,
        }
    }
}

impl ElementWarnings<'_> {
    pub fn malformed_problem(&self) -> Option<ElementProblem> {
        self.malformed
    }

    pub fn repetition(&self) -> Option<&RepeatedElement> {
        self.repetition
    }

    pub fn is_contradictory(&self) -> bool {
        self.repetition.is_some_and(RepeatedElement::contradictory)
    }

    /// True for malformed values or any repeated singleton, including
    /// identical payloads that violate singleton cardinality.
    pub fn has_warning(&self) -> bool {
        self.malformed.is_some()
            || self
                .repetition
                .is_some_and(RepeatedElement::violates_singleton_cardinality)
    }
}

impl<'before, 'after> ElementChange<'before, 'after> {
    pub fn kind(&self) -> ElementChangeKind {
        self.kind
    }

    /// Zero-based occurrence among IEs with the same numeric ID and, for
    /// extension IEs, the same extension ID.
    pub fn occurrence(&self) -> usize {
        self.occurrence
    }

    pub fn before(&self) -> Option<ExplorerElement<'before>> {
        self.before
    }

    pub fn after(&self) -> Option<ExplorerElement<'after>> {
        self.after
    }
}

impl<'before, 'after> InformationElementDiff<'before, 'after> {
    pub fn changes(&self) -> &[ElementChange<'before, 'after>] {
        &self.changes
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn subtype_changed(&self) -> bool {
        self.subtype_changed
    }

    /// True if the ordered sequence of raw IE TLVs differs, including changes
    /// caused only by IE ordering. This does not include frame-header changes.
    pub fn wire_ie_sequence_changed(&self) -> bool {
        self.wire_ie_sequence_changed
    }
}

impl ResourceUsage {
    pub fn work_units(self) -> usize {
        self.work_units
    }
    pub fn allocation_bytes(self) -> usize {
        self.allocation_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    LimitExceeded(&'static str),
    Truncated(&'static str),
    UnsupportedProtocolVersion(u8),
    UnsupportedFrameType(u8),
    UnsupportedManagementSubtype(u8),
    InvalidManagementDsFlags(u8),
    InvalidFcs { expected: u32, actual: u32 },
    TruncatedInformationElement { offset: u32, declared_length: u8 },
    EmptyExtensionElement { offset: u32 },
    CanonicalMagic,
    CanonicalVersion(u16),
    CanonicalFraming(u8),
    CanonicalLength,
    NonCanonical,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

struct Control<'a, C> {
    limits: ParseLimits,
    cancellation: &'a C,
    work: usize,
    allocation: usize,
}

impl<C: Cancellation> Control<'_, C> {
    fn charge(&mut self, amount: usize) -> Result<(), Error> {
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.work = self
            .work
            .checked_add(amount)
            .ok_or(Error::LimitExceeded("work units"))?;
        if self.work > self.limits.max_work_units {
            return Err(Error::LimitExceeded("work units"));
        }
        Ok(())
    }

    fn allocate(&mut self, amount: usize) -> Result<(), Error> {
        self.charge(1)?;
        self.allocation = self
            .allocation
            .checked_add(amount)
            .ok_or(Error::LimitExceeded("allocation bytes"))?;
        if self.allocation > self.limits.max_allocation_bytes {
            return Err(Error::LimitExceeded("allocation bytes"));
        }
        Ok(())
    }

    fn usage(&self) -> ResourceUsage {
        ResourceUsage {
            work_units: self.work,
            allocation_bytes: self.allocation,
        }
    }
}

pub fn parse(input: &[u8], framing: InputFraming) -> Result<ManagementFrame, Error> {
    parse_with(input, framing, ParseLimits::default(), &NeverCancel)
}

pub fn parse_with<C: Cancellation>(
    input: &[u8],
    framing: InputFraming,
    limits: ParseLimits,
    cancellation: &C,
) -> Result<ManagementFrame, Error> {
    parse_with_usage(input, framing, limits, cancellation).map(|(frame, _)| frame)
}

pub fn parse_with_usage<C: Cancellation>(
    input: &[u8],
    framing: InputFraming,
    limits: ParseLimits,
    cancellation: &C,
) -> Result<(ManagementFrame, ResourceUsage), Error> {
    let mut control = Control {
        limits,
        cancellation,
        work: 0,
        allocation: 0,
    };
    let frame = parse_controlled(input, framing, &mut control)?;
    Ok((frame, control.usage()))
}

pub fn diff_information_elements<'before, 'after>(
    before: &'before ManagementFrame,
    after: &'after ManagementFrame,
) -> Result<InformationElementDiff<'before, 'after>, Error> {
    diff_information_elements_with_usage(before, after, ParseLimits::default(), &NeverCancel)
        .map(|(diff, _)| diff)
}

/// Compare raw IE payloads while preserving every repeated occurrence. The
/// returned change list is bounded by the sum of both frames' IE counts.
pub fn diff_information_elements_with_usage<'before, 'after, C: Cancellation>(
    before: &'before ManagementFrame,
    after: &'after ManagementFrame,
    limits: ParseLimits,
    cancellation: &C,
) -> Result<(InformationElementDiff<'before, 'after>, ResourceUsage), Error> {
    let mut control = Control {
        limits,
        cancellation,
        work: 0,
        allocation: 0,
    };
    let before_occurrences = index_occurrences(before, &mut control)?;
    let after_occurrences = index_occurrences(after, &mut control)?;
    let wire_ie_sequence_changed = ordered_ie_sequence_changed(before, after, &mut control)?;
    let max_changes = before_occurrences
        .len()
        .checked_add(after_occurrences.len())
        .ok_or(Error::LimitExceeded("element count"))?;
    control.allocate(
        max_changes
            .checked_mul(std::mem::size_of::<ElementChange<'before, 'after>>())
            .ok_or(Error::LimitExceeded("allocation bytes"))?,
    )?;
    let mut changes = Vec::with_capacity(max_changes);
    let (mut before_index, mut after_index) = (0, 0);
    while before_index < before_occurrences.len() || after_index < after_occurrences.len() {
        control.charge(1)?;
        let ordering = match (
            before_occurrences.get(before_index),
            after_occurrences.get(after_index),
        ) {
            (Some(before), Some(after)) => occurrence_order(before, after),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };
        match ordering {
            std::cmp::Ordering::Less => {
                let occurrence = before_occurrences[before_index];
                changes.push(ElementChange {
                    kind: ElementChangeKind::Removed,
                    occurrence: occurrence.occurrence,
                    before: Some(ExplorerElement {
                        frame: before,
                        index: occurrence.index,
                    }),
                    after: None,
                });
                before_index += 1;
            }
            std::cmp::Ordering::Greater => {
                let occurrence = after_occurrences[after_index];
                changes.push(ElementChange {
                    kind: ElementChangeKind::Added,
                    occurrence: occurrence.occurrence,
                    before: None,
                    after: Some(ExplorerElement {
                        frame: after,
                        index: occurrence.index,
                    }),
                });
                after_index += 1;
            }
            std::cmp::Ordering::Equal => {
                let before_occurrence = before_occurrences[before_index];
                let after_occurrence = after_occurrences[after_index];
                let before_element = &before.elements[before_occurrence.index];
                let after_element = &after.elements[after_occurrence.index];
                control.charge(
                    before_element
                        .payload
                        .len()
                        .max(after_element.payload.len())
                        .max(1),
                )?;
                if before_element.payload != after_element.payload {
                    changes.push(ElementChange {
                        kind: ElementChangeKind::Modified,
                        occurrence: before_occurrence.occurrence,
                        before: Some(ExplorerElement {
                            frame: before,
                            index: before_occurrence.index,
                        }),
                        after: Some(ExplorerElement {
                            frame: after,
                            index: after_occurrence.index,
                        }),
                    });
                }
                before_index += 1;
                after_index += 1;
            }
        }
    }
    Ok((
        InformationElementDiff {
            changes,
            subtype_changed: before.subtype != after.subtype,
            wire_ie_sequence_changed,
        },
        control.usage(),
    ))
}

fn index_occurrences<C: Cancellation>(
    frame: &ManagementFrame,
    control: &mut Control<'_, C>,
) -> Result<Vec<ElementOccurrence>, Error> {
    if frame.raw_mpdu.len() > control.limits.max_frame_bytes {
        return Err(Error::LimitExceeded("frame bytes"));
    }
    if frame.elements.len() > control.limits.max_elements {
        return Err(Error::LimitExceeded("element count"));
    }
    control.allocate(
        frame
            .elements
            .len()
            .checked_mul(std::mem::size_of::<ElementOccurrence>())
            .ok_or(Error::LimitExceeded("allocation bytes"))?,
    )?;
    let mut occurrences = Vec::with_capacity(frame.elements.len());
    let mut counts = [0_usize; 511];
    let mut total_payload = 0_usize;
    for (index, element) in frame.elements.iter().enumerate() {
        control.charge(1)?;
        total_payload = total_payload
            .checked_add(element.payload.len())
            .ok_or(Error::LimitExceeded("IE payload bytes"))?;
        if total_payload > control.limits.max_ie_payload_bytes {
            return Err(Error::LimitExceeded("IE payload bytes"));
        }
        let key = ElementKey {
            id: element.id,
            extension_id: element.extension_id,
        };
        let slot = element_slot(key, element.offset)?;
        let occurrence = counts[slot];
        counts[slot] = occurrence
            .checked_add(1)
            .ok_or(Error::LimitExceeded("element count"))?;
        occurrences.push(ElementOccurrence {
            key,
            occurrence,
            index,
        });
    }
    heap_sort_occurrences(&mut occurrences, control)?;
    Ok(occurrences)
}

fn element_slot(key: ElementKey, offset: u32) -> Result<usize, Error> {
    if key.id == 255 {
        key.extension_id
            .map(|extension_id| 255 + usize::from(extension_id))
            .ok_or(Error::EmptyExtensionElement { offset })
    } else {
        Ok(usize::from(key.id))
    }
}

fn occurrence_order(left: &ElementOccurrence, right: &ElementOccurrence) -> std::cmp::Ordering {
    left.key
        .cmp(&right.key)
        .then_with(|| left.occurrence.cmp(&right.occurrence))
}

fn heap_sort_occurrences<C: Cancellation>(
    occurrences: &mut [ElementOccurrence],
    control: &mut Control<'_, C>,
) -> Result<(), Error> {
    for root in (0..occurrences.len() / 2).rev() {
        sift_occurrence_heap(occurrences, root, occurrences.len(), control)?;
    }
    for end in (1..occurrences.len()).rev() {
        control.charge(1)?;
        occurrences.swap(0, end);
        sift_occurrence_heap(occurrences, 0, end, control)?;
    }
    Ok(())
}

fn sift_occurrence_heap<C: Cancellation>(
    occurrences: &mut [ElementOccurrence],
    mut root: usize,
    heap_len: usize,
    control: &mut Control<'_, C>,
) -> Result<(), Error> {
    loop {
        let child = root
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or(Error::LimitExceeded("work units"))?;
        if child >= heap_len {
            return Ok(());
        }
        let mut largest = child;
        if child + 1 < heap_len {
            control.charge(1)?;
            if occurrence_order(&occurrences[largest], &occurrences[child + 1])
                == std::cmp::Ordering::Less
            {
                largest = child + 1;
            }
        }
        control.charge(1)?;
        if occurrence_order(&occurrences[root], &occurrences[largest]) != std::cmp::Ordering::Less {
            return Ok(());
        }
        control.charge(1)?;
        occurrences.swap(root, largest);
        root = largest;
    }
}

fn ordered_ie_sequence_changed<C: Cancellation>(
    before: &ManagementFrame,
    after: &ManagementFrame,
    control: &mut Control<'_, C>,
) -> Result<bool, Error> {
    if before.elements.len() != after.elements.len() {
        return Ok(true);
    }
    for (before, after) in before.elements.iter().zip(&after.elements) {
        control.charge(before.payload.len().max(after.payload.len()).max(1))?;
        if before.id != after.id || before.payload != after.payload {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_controlled<C: Cancellation>(
    input: &[u8],
    framing: InputFraming,
    control: &mut Control<'_, C>,
) -> Result<ManagementFrame, Error> {
    let limits = control.limits;
    control.charge(1)?;
    if input.len() > limits.max_frame_bytes {
        return Err(Error::LimitExceeded("frame bytes"));
    }
    let mac_len = match framing {
        InputFraming::FcsAbsent => input.len(),
        InputFraming::FcsPresentAndValidate => {
            let mac_len = input.len().checked_sub(4).ok_or(Error::Truncated("FCS"))?;
            let actual = read_u32(input, mac_len).ok_or(Error::Truncated("FCS"))?;
            let expected = crc32(&input[..mac_len], control)?;
            if actual != expected {
                return Err(Error::InvalidFcs { expected, actual });
            }
            mac_len
        }
    };
    if mac_len < HEADER_LEN {
        return Err(Error::Truncated("management header"));
    }
    control.charge(HEADER_LEN)?;
    let frame_control = read_u16(input, 0).ok_or(Error::Truncated("frame control"))?;
    let protocol = (frame_control & 0b11) as u8;
    if protocol != 0 {
        return Err(Error::UnsupportedProtocolVersion(protocol));
    }
    let frame_type = ((frame_control >> 2) & 0b11) as u8;
    if frame_type != 0 {
        return Err(Error::UnsupportedFrameType(frame_type));
    }
    let subtype_raw = ((frame_control >> 4) & 0x0f) as u8;
    let subtype = match subtype_raw {
        4 => ManagementSubtype::ProbeRequest,
        5 => ManagementSubtype::ProbeResponse,
        8 => ManagementSubtype::Beacon,
        value => return Err(Error::UnsupportedManagementSubtype(value)),
    };
    let ds_flags = ((frame_control >> 8) & 0x03) as u8;
    if ds_flags != 0 {
        return Err(Error::InvalidManagementDsFlags(ds_flags));
    }
    let fixed_len = if subtype == ManagementSubtype::ProbeRequest {
        0
    } else {
        RESPONSE_FIXED_LEN
    };
    let elements_offset = HEADER_LEN
        .checked_add(fixed_len)
        .ok_or(Error::LimitExceeded("offset"))?;
    if mac_len < elements_offset {
        return Err(Error::Truncated("management fixed fields"));
    }
    let address = |offset: usize| {
        let mut out = [0_u8; 6];
        out.copy_from_slice(&input[offset..offset + 6]);
        MacAddress(out)
    };
    let sequence_control = read_u16(input, 22).ok_or(Error::Truncated("sequence control"))?;
    let fixed = if fixed_len == 0 {
        None
    } else {
        Some(ResponseFixedFields {
            timestamp: read_u64(input, HEADER_LEN).ok_or(Error::Truncated("timestamp"))?,
            beacon_interval_tu: read_u16(input, HEADER_LEN + 8)
                .ok_or(Error::Truncated("beacon interval"))?,
            capability_information: read_u16(input, HEADER_LEN + 10)
                .ok_or(Error::Truncated("capability"))?,
        })
    };

    let element_count = preflight_elements(input, elements_offset, mac_len, control)?;
    control.allocate(
        element_count
            .checked_mul(std::mem::size_of::<InformationElement>())
            .ok_or(Error::LimitExceeded("allocation bytes"))?,
    )?;
    let mut elements = Vec::with_capacity(element_count);
    let mut cursor = elements_offset;
    let mut total_payload = 0_usize;
    while cursor < mac_len {
        control.charge(1)?;
        if elements.len() >= limits.max_elements {
            return Err(Error::LimitExceeded("element count"));
        }
        let header_end = cursor
            .checked_add(2)
            .ok_or(Error::LimitExceeded("offset"))?;
        if header_end > mac_len {
            return Err(Error::TruncatedInformationElement {
                offset: to_u32(cursor)?,
                declared_length: 0,
            });
        }
        let id = input[cursor];
        let length = input[cursor + 1];
        let payload_end = header_end
            .checked_add(usize::from(length))
            .ok_or(Error::LimitExceeded("offset"))?;
        if payload_end > mac_len {
            return Err(Error::TruncatedInformationElement {
                offset: to_u32(cursor)?,
                declared_length: length,
            });
        }
        total_payload = total_payload
            .checked_add(usize::from(length))
            .ok_or(Error::LimitExceeded("IE payload bytes"))?;
        if total_payload > limits.max_ie_payload_bytes {
            return Err(Error::LimitExceeded("IE payload bytes"));
        }
        control.charge(usize::from(length))?;
        let payload = &input[header_end..payload_end];
        if id == 255 && payload.is_empty() {
            return Err(Error::EmptyExtensionElement {
                offset: to_u32(cursor)?,
            });
        }
        let extension_id = (id == 255).then(|| payload[0]);
        control.allocate(payload.len())?;
        let decoded = decode_element(id, payload, subtype, control)?;
        elements.push(InformationElement {
            id,
            extension_id,
            offset: to_u32(cursor)?,
            payload_offset: to_u32(header_end)?,
            payload: payload.to_vec(),
            decoded,
        });
        cursor = payload_end;
    }
    let repeated_elements = summarize_repeats(&elements, control)?;
    control.allocate(input.len())?;
    copy_work(input.len(), control)?;
    Ok(ManagementFrame {
        schema_version: 1,
        framing,
        raw_mpdu: input.to_vec(),
        subtype,
        frame_control,
        duration_id: read_u16(input, 2).ok_or(Error::Truncated("duration"))?,
        addresses: AddressRoles {
            receiver: address(4),
            transmitter: address(10),
            bssid_field: address(16),
        },
        sequence_number: sequence_control >> 4,
        fragment_number: (sequence_control & 0x0f) as u8,
        fixed,
        elements,
        repeated_elements,
    })
}

fn preflight_elements<C: Cancellation>(
    input: &[u8],
    mut cursor: usize,
    mac_len: usize,
    control: &mut Control<'_, C>,
) -> Result<usize, Error> {
    let mut count = 0_usize;
    let mut total_payload = 0_usize;
    while cursor < mac_len {
        control.charge(1)?;
        count = count
            .checked_add(1)
            .ok_or(Error::LimitExceeded("element count"))?;
        if count > control.limits.max_elements {
            return Err(Error::LimitExceeded("element count"));
        }
        let header_end = cursor
            .checked_add(2)
            .ok_or(Error::LimitExceeded("offset"))?;
        if header_end > mac_len {
            return Err(Error::TruncatedInformationElement {
                offset: to_u32(cursor)?,
                declared_length: 0,
            });
        }
        let length = input[cursor + 1];
        let payload_end = header_end
            .checked_add(usize::from(length))
            .ok_or(Error::LimitExceeded("offset"))?;
        if payload_end > mac_len {
            return Err(Error::TruncatedInformationElement {
                offset: to_u32(cursor)?,
                declared_length: length,
            });
        }
        if input[cursor] == 255 && length == 0 {
            return Err(Error::EmptyExtensionElement {
                offset: to_u32(cursor)?,
            });
        }
        total_payload = total_payload
            .checked_add(usize::from(length))
            .ok_or(Error::LimitExceeded("IE payload bytes"))?;
        if total_payload > control.limits.max_ie_payload_bytes {
            return Err(Error::LimitExceeded("IE payload bytes"));
        }
        control.charge(usize::from(length))?;
        cursor = payload_end;
    }
    Ok(count)
}

impl ManagementFrame {
    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }
    pub fn framing(&self) -> InputFraming {
        self.framing
    }
    pub fn raw_mpdu(&self) -> &[u8] {
        &self.raw_mpdu
    }
    pub fn subtype(&self) -> ManagementSubtype {
        self.subtype
    }
    pub fn frame_control(&self) -> u16 {
        self.frame_control
    }
    pub fn duration_id(&self) -> u16 {
        self.duration_id
    }
    pub fn addresses(&self) -> &AddressRoles {
        &self.addresses
    }
    pub fn sequence_number(&self) -> u16 {
        self.sequence_number
    }
    pub fn fragment_number(&self) -> u8 {
        self.fragment_number
    }
    pub fn fixed(&self) -> Option<ResponseFixedFields> {
        self.fixed
    }
    pub fn elements(&self) -> &[InformationElement] {
        &self.elements
    }
    pub fn repeated_elements(&self) -> &[RepeatedElement] {
        &self.repeated_elements
    }
    pub fn ie_explorer(&self) -> InformationElementExplorer<'_> {
        InformationElementExplorer { frame: self }
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        self.canonical_bytes_with(ParseLimits::default(), &NeverCancel)
    }

    pub fn canonical_bytes_with<C: Cancellation>(
        &self,
        limits: ParseLimits,
        cancellation: &C,
    ) -> Result<Vec<u8>, Error> {
        self.canonical_bytes_with_usage(limits, cancellation)
            .map(|(bytes, _)| bytes)
    }

    pub fn canonical_bytes_with_usage<C: Cancellation>(
        &self,
        limits: ParseLimits,
        cancellation: &C,
    ) -> Result<(Vec<u8>, ResourceUsage), Error> {
        if self.raw_mpdu.len() > limits.max_frame_bytes {
            return Err(Error::LimitExceeded("frame bytes"));
        }
        let mut control = Control {
            limits,
            cancellation,
            work: 0,
            allocation: 0,
        };
        let length = CANONICAL_MAGIC
            .len()
            .checked_add(2)
            .and_then(|n| n.checked_add(1))
            .and_then(|n| n.checked_add(4))
            .and_then(|n| n.checked_add(self.raw_mpdu.len()))
            .ok_or(Error::LimitExceeded("canonical bytes"))?;
        if length > limits.max_canonical_bytes {
            return Err(Error::LimitExceeded("canonical bytes"));
        }
        control.allocate(length)?;
        let mut out = Vec::with_capacity(length);
        control.charge(14)?;
        out.extend_from_slice(CANONICAL_MAGIC);
        out.extend_from_slice(&CANONICAL_VERSION.to_le_bytes());
        out.push(match self.framing {
            InputFraming::FcsAbsent => 0,
            InputFraming::FcsPresentAndValidate => 1,
        });
        out.extend_from_slice(&to_u32(self.raw_mpdu.len())?.to_le_bytes());
        for chunk in self.raw_mpdu.chunks(256) {
            control.charge(chunk.len())?;
            out.extend_from_slice(chunk);
        }
        Ok((out, control.usage()))
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, Error> {
        Self::from_canonical_bytes_with(bytes, ParseLimits::default(), &NeverCancel)
    }

    pub fn from_canonical_bytes_with<C: Cancellation>(
        bytes: &[u8],
        limits: ParseLimits,
        cancellation: &C,
    ) -> Result<Self, Error> {
        Self::from_canonical_bytes_with_usage(bytes, limits, cancellation).map(|(frame, _)| frame)
    }

    pub fn from_canonical_bytes_with_usage<C: Cancellation>(
        bytes: &[u8],
        limits: ParseLimits,
        cancellation: &C,
    ) -> Result<(Self, ResourceUsage), Error> {
        let mut control = Control {
            limits,
            cancellation,
            work: 0,
            allocation: 0,
        };
        if bytes.len() > limits.max_canonical_bytes {
            return Err(Error::LimitExceeded("canonical bytes"));
        }
        control.charge(14)?;
        if bytes.len() < 14 || bytes.get(..7) != Some(CANONICAL_MAGIC.as_slice()) {
            return Err(Error::CanonicalMagic);
        }
        let version = read_u16(bytes, 7).ok_or(Error::CanonicalLength)?;
        if version != CANONICAL_VERSION {
            return Err(Error::CanonicalVersion(version));
        }
        let framing = match bytes[9] {
            0 => InputFraming::FcsAbsent,
            1 => InputFraming::FcsPresentAndValidate,
            value => return Err(Error::CanonicalFraming(value)),
        };
        let declared = read_u32(bytes, 10).ok_or(Error::CanonicalLength)? as usize;
        let end = 14_usize
            .checked_add(declared)
            .ok_or(Error::CanonicalLength)?;
        if end != bytes.len() {
            return Err(Error::CanonicalLength);
        }
        let parsed = parse_controlled(&bytes[14..], framing, &mut control)?;
        Ok((parsed, control.usage()))
    }
}

fn decode_element<C: Cancellation>(
    id: u8,
    payload: &[u8],
    subtype: ManagementSubtype,
    control: &mut Control<'_, C>,
) -> Result<ElementDecode, Error> {
    control.charge(payload.len().max(1))?;
    Ok(match id {
        0 if payload.len() > 32 => ElementDecode::Malformed(ElementProblem::SsidTooLong),
        0 if payload.is_empty() => {
            ElementDecode::Ssid(if subtype == ManagementSubtype::ProbeRequest {
                SsidValue::Wildcard
            } else {
                SsidValue::Hidden
            })
        }
        0 => {
            control.allocate(payload.len())?;
            ElementDecode::Ssid(SsidValue::Binary(payload.to_vec()))
        }
        1 if payload.is_empty() || payload.len() > 8 => {
            ElementDecode::Malformed(ElementProblem::RatesLength)
        }
        1 => ElementDecode::SupportedRates(decode_rates(payload, control)?),
        50 if payload.is_empty() => ElementDecode::Malformed(ElementProblem::RatesLength),
        50 => ElementDecode::ExtendedSupportedRates(decode_rates(payload, control)?),
        3 if payload.len() != 1 => ElementDecode::Malformed(ElementProblem::DsLength),
        3 => ElementDecode::DsParameterChannel(payload[0]),
        5 if subtype != ManagementSubtype::Beacon => {
            ElementDecode::Malformed(ElementProblem::TimContext)
        }
        5 if payload.len() < 4 => ElementDecode::Malformed(ElementProblem::TimTooShort),
        5 if payload.len() > 254 => ElementDecode::Malformed(ElementProblem::TimTooLong),
        5 if payload[1] == 0 => ElementDecode::Malformed(ElementProblem::TimInvalidPeriod),
        5 if payload[0] >= payload[1] => {
            ElementDecode::Malformed(ElementProblem::TimCountExceedsPeriod)
        }
        5 => {
            control.allocate(payload.len() - 3)?;
            ElementDecode::Tim(TimStructure {
                dtim_count: payload[0],
                dtim_period: payload[1],
                bitmap_control: payload[2],
                partial_virtual_bitmap: payload[3..].to_vec(),
            })
        }
        7 => decode_country(payload, control)?,
        255 => {
            control.allocate(payload.len() - 1)?;
            ElementDecode::Extension {
                extension_id: payload[0],
                body: payload[1..].to_vec(),
            }
        }
        _ => ElementDecode::Unknown,
    })
}

fn decode_rates<C: Cancellation>(
    payload: &[u8],
    control: &mut Control<'_, C>,
) -> Result<Vec<EncodedRate>, Error> {
    control.allocate(
        payload
            .len()
            .checked_mul(std::mem::size_of::<EncodedRate>())
            .ok_or(Error::LimitExceeded("allocation bytes"))?,
    )?;
    let mut rates = Vec::with_capacity(payload.len());
    for raw in payload {
        rates.push(EncodedRate {
            raw: *raw,
            basic: raw & 0x80 != 0,
            units_500_kbps: raw & 0x7f,
        });
    }
    Ok(rates)
}

fn decode_country<C: Cancellation>(
    payload: &[u8],
    control: &mut Control<'_, C>,
) -> Result<ElementDecode, Error> {
    if payload.len() < 6 {
        return Ok(ElementDecode::Malformed(ElementProblem::CountryTooShort));
    }
    if !payload.len().is_multiple_of(2) {
        return Ok(ElementDecode::Malformed(
            ElementProblem::CountryInvalidPadding,
        ));
    }
    let remainder = payload.len() - 3;
    let (triplet_bytes, padding) = match remainder % 3 {
        0 => (&payload[3..], None),
        1 if payload[payload.len() - 1] == 0 => (&payload[3..payload.len() - 1], Some(0)),
        _ => {
            return Ok(ElementDecode::Malformed(
                ElementProblem::CountryInvalidPadding,
            ));
        }
    };
    let mut header = [0; 3];
    header.copy_from_slice(&payload[..3]);
    control.allocate(triplet_bytes.len())?;
    let mut triplets = Vec::with_capacity(triplet_bytes.len() / 3);
    let mut offset = 0;
    while offset < triplet_bytes.len() {
        triplets.push([
            triplet_bytes[offset],
            triplet_bytes[offset + 1],
            triplet_bytes[offset + 2],
        ]);
        offset += 3;
    }
    Ok(ElementDecode::Country(CountryStructure {
        country_environment: header,
        triplets,
        padding,
    }))
}

fn summarize_repeats<C: Cancellation>(
    elements: &[InformationElement],
    control: &mut Control<'_, C>,
) -> Result<Vec<RepeatedElement>, Error> {
    // IDs 0..=254 use slots 0..=254. Extension elements use slots 255..=510.
    // Fixed stack maps plus exact-sized result vectors keep grouping linear and
    // prevent allocator growth from escaping the cumulative byte accounting.
    control.allocate(511 * std::mem::size_of::<usize>() * 2)?;
    let mut counts = [0_usize; 511];
    for (index, element) in elements.iter().enumerate() {
        control.charge(1)?;
        let slot = if element.id == 255 {
            255 + usize::from(element.extension_id.ok_or(Error::EmptyExtensionElement {
                offset: element.offset,
            })?)
        } else {
            usize::from(element.id)
        };
        counts[slot] = counts[slot]
            .checked_add(1)
            .ok_or(Error::LimitExceeded("element count"))?;
        let _ = index;
    }
    let repeated_count = counts.iter().filter(|count| **count >= 2).count();
    control.allocate(repeated_count * std::mem::size_of::<RepeatedElement>())?;
    let index_count = counts.iter().filter(|count| **count >= 2).sum::<usize>();
    control.allocate(index_count * std::mem::size_of::<u32>())?;
    let mut repeated = Vec::with_capacity(repeated_count);
    let mut slot_to_repeat = [usize::MAX; 511];
    for (slot, count) in counts.iter().copied().enumerate() {
        if count < 2 {
            continue;
        }
        let key = if slot < 255 {
            ElementKey {
                id: slot as u8,
                extension_id: None,
            }
        } else {
            ElementKey {
                id: 255,
                extension_id: Some((slot - 255) as u8),
            }
        };
        let singleton = matches!(key.id, 0 | 1 | 3 | 5 | 7 | 50);
        slot_to_repeat[slot] = repeated.len();
        repeated.push(RepeatedElement {
            key,
            indices: Vec::with_capacity(count),
            payloads_identical: true,
            violates_singleton_cardinality: singleton,
            contradictory: false,
        });
    }
    for (index, element) in elements.iter().enumerate() {
        control.charge(1)?;
        let slot = if element.id == 255 {
            255 + usize::from(element.extension_id.unwrap())
        } else {
            usize::from(element.id)
        };
        let repeat_index = slot_to_repeat[slot];
        if repeat_index == usize::MAX {
            continue;
        }
        let group = &mut repeated[repeat_index];
        if let Some(first_index) = group.indices.first().copied() {
            let first = &elements[first_index as usize].payload;
            control.charge(first.len().max(element.payload.len()))?;
            if first != &element.payload {
                group.payloads_identical = false;
            }
        }
        group.indices.push(to_u32(index)?);
    }
    for group in &mut repeated {
        group.contradictory = group.violates_singleton_cardinality && !group.payloads_identical;
    }
    Ok(repeated)
}

fn copy_work<C: Cancellation>(bytes: usize, control: &mut Control<'_, C>) -> Result<(), Error> {
    for chunk in (0..bytes).step_by(256) {
        control.charge(256.min(bytes - chunk))?;
    }
    Ok(())
}

fn crc32<C: Cancellation>(bytes: &[u8], control: &mut Control<'_, C>) -> Result<u32, Error> {
    let mut crc = u32::MAX;
    for (index, byte) in bytes.iter().enumerate() {
        if index % 64 == 0 {
            control.charge(64.min(bytes.len() - index))?;
        }
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    Ok(!crc)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn to_u32(value: usize) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::LimitExceeded("offset"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn header(subtype: u8) -> Vec<u8> {
        let mut out = vec![subtype << 4, 0, 0x34, 0x12];
        out.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        out.extend_from_slice(&[7, 8, 9, 10, 11, 12]);
        out.extend_from_slice(&[13, 14, 15, 16, 17, 18]);
        out.extend_from_slice(&0xabcd_u16.to_le_bytes());
        out
    }

    fn beacon() -> Vec<u8> {
        let mut out = header(8);
        out.extend_from_slice(&0x0807_0605_0403_0201_u64.to_le_bytes());
        out.extend_from_slice(&100_u16.to_le_bytes());
        out.extend_from_slice(&0x0411_u16.to_le_bytes());
        out.extend_from_slice(&[0, 0, 1, 2, 0x82, 0x0c, 3, 1, 11, 5, 4, 0, 2, 0, 1]);
        out
    }

    #[test]
    fn beacon_preserves_header_fixed_fields_and_ie_offsets() {
        let frame = parse(&beacon(), InputFraming::FcsAbsent).unwrap();
        assert_eq!(frame.subtype, ManagementSubtype::Beacon);
        assert_eq!(frame.addresses.receiver.0, [1, 2, 3, 4, 5, 6]);
        assert_eq!(frame.addresses.transmitter.0, [7, 8, 9, 10, 11, 12]);
        assert_eq!(frame.addresses.bssid_field.0, [13, 14, 15, 16, 17, 18]);
        assert_eq!(frame.sequence_number, 0xabc);
        assert_eq!(frame.fragment_number, 0xd);
        assert_eq!(frame.fixed.unwrap().timestamp, 0x0807_0605_0403_0201);
        assert_eq!(frame.elements[0].offset, 36);
        assert_eq!(frame.elements[0].payload_offset, 38);
        assert_eq!(
            frame.elements[0].decoded,
            ElementDecode::Ssid(SsidValue::Hidden)
        );
        assert!(matches!(frame.elements[3].decoded, ElementDecode::Tim(_)));
    }

    #[test]
    fn probe_request_has_wildcard_ssid_and_does_not_relabel_address_three() {
        let mut raw = header(4);
        raw.extend_from_slice(&[0, 0, 221, 3, 1, 2, 3]);
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        assert_eq!(frame.fixed, None);
        assert_eq!(
            frame.elements[0].decoded,
            ElementDecode::Ssid(SsidValue::Wildcard)
        );
        assert_eq!(frame.addresses.bssid_field.0, [13, 14, 15, 16, 17, 18]);
    }

    #[test]
    fn probe_response_uses_fixed_fields_and_binary_ssid() {
        let mut raw = header(5);
        raw.extend_from_slice(&[0; 12]);
        raw.extend_from_slice(&[0, 3, 0xff, 0, b'x']);
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        assert_eq!(frame.subtype, ManagementSubtype::ProbeResponse);
        assert_eq!(
            frame.elements[0].decoded,
            ElementDecode::Ssid(SsidValue::Binary(vec![0xff, 0, b'x']))
        );
    }

    #[test]
    fn fcs_is_explicit_and_validated_without_autodetection() {
        let body = beacon();
        let expected = {
            let mut control = Control {
                limits: ParseLimits::default(),
                cancellation: &NeverCancel,
                work: 0,
                allocation: 0,
            };
            crc32(&body, &mut control).unwrap()
        };
        let mut with_fcs = body.clone();
        with_fcs.extend_from_slice(&expected.to_le_bytes());
        assert!(parse(&with_fcs, InputFraming::FcsPresentAndValidate).is_ok());
        assert!(matches!(
            parse(&body, InputFraming::FcsPresentAndValidate),
            Err(Error::InvalidFcs { .. })
        ));
        if let Ok(unstripped) = parse(&with_fcs, InputFraming::FcsAbsent) {
            assert_eq!(unstripped.raw_mpdu, with_fcs);
        }
    }

    #[test]
    fn typed_foundation_retains_raw_rates_ds_country_extension_and_unknown() {
        let mut raw = header(8);
        raw.extend_from_slice(&[0; 12]);
        raw.extend_from_slice(&[1, 2, 0x82, 0x0c, 50, 1, 0x96, 3, 1, 36]);
        raw.extend_from_slice(&[7, 6, b'U', b'S', b' ', 1, 11, 20]);
        raw.extend_from_slice(&[255, 3, 35, 9, 8, 199, 2, 7, 6]);
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        assert_eq!(
            frame.elements[0].decoded,
            ElementDecode::SupportedRates(vec![
                EncodedRate {
                    raw: 0x82,
                    basic: true,
                    units_500_kbps: 2
                },
                EncodedRate {
                    raw: 0x0c,
                    basic: false,
                    units_500_kbps: 12
                }
            ])
        );
        assert_eq!(
            frame.elements[2].decoded,
            ElementDecode::DsParameterChannel(36)
        );
        assert!(matches!(
            frame.elements[3].decoded,
            ElementDecode::Country(_)
        ));
        assert_eq!(frame.elements[4].extension_id, Some(35));
        assert!(matches!(frame.elements[5].decoded, ElementDecode::Unknown));
    }

    fn probe_request_with(elements: &[u8]) -> ManagementFrame {
        let mut raw = header(4);
        raw.extend_from_slice(elements);
        parse(&raw, InputFraming::FcsAbsent).unwrap()
    }

    #[test]
    fn explorer_view_preserves_raw_order_unknown_identity_and_warnings() {
        let frame = probe_request_with(&[
            221, 2, 0xaa, 0xbb, // unknown decoder ID
            3, 2, 1, 2, // malformed DS element
            0, 1, b'a', 0, 1, b'b', // contradictory singleton SSIDs
        ]);
        let explorer = frame.ie_explorer();
        assert_eq!(explorer.subtype(), ManagementSubtype::ProbeRequest);
        assert_eq!(explorer.raw_mpdu(), frame.raw_mpdu());
        let elements = explorer.elements().collect::<Vec<_>>();
        assert_eq!(elements.len(), 4);
        assert_eq!(elements[0].index(), 0);
        assert_eq!(elements[0].raw_bytes(), &[221, 2, 0xaa, 0xbb]);
        assert_eq!(elements[0].raw_payload(), &[0xaa, 0xbb]);
        assert_eq!(
            elements[0].standards_reference(),
            StandardsElementReference::Ieee80211ElementIdentifier {
                id: 221,
                extension_id: None
            }
        );
        assert!(elements[0].is_unrecognized_by_decoder());
        assert!(!elements[0].warnings().has_warning());
        assert_eq!(
            elements[1].warnings().malformed_problem(),
            Some(ElementProblem::DsLength)
        );
        assert!(elements[1].warnings().has_warning());
        assert!(elements[2].warnings().is_contradictory());
        assert!(elements[3].warnings().is_contradictory());
        assert_eq!(
            elements[2].warnings().repetition().unwrap().indices(),
            &[2, 3]
        );
    }

    #[test]
    fn explorer_reports_extension_identity_without_inventing_clause_citations() {
        let frame = probe_request_with(&[255, 3, 35, 0xaa, 0xbb]);
        let element = frame.ie_explorer().elements().next().unwrap();
        assert_eq!(
            element.standards_reference(),
            StandardsElementReference::Ieee80211ElementIdentifier {
                id: 255,
                extension_id: Some(35)
            }
        );
        assert_eq!(element.raw_bytes(), &[255, 3, 35, 0xaa, 0xbb]);
        assert_eq!(element.raw_payload(), &[35, 0xaa, 0xbb]);
        assert!(matches!(element.decoded(), ElementDecode::Extension { .. }));
    }

    #[test]
    fn explorer_warns_on_identical_singleton_repeats_but_not_allowed_vendor_repeats() {
        let singleton = probe_request_with(&[0, 1, b'a', 0, 1, b'a']);
        let views = singleton.ie_explorer().elements().collect::<Vec<_>>();
        assert_eq!(views.len(), 2);
        for view in views {
            let warnings = view.warnings();
            let repetition = warnings.repetition().unwrap();
            assert!(repetition.violates_singleton_cardinality());
            assert!(repetition.payloads_identical());
            assert!(!warnings.is_contradictory());
            assert!(warnings.has_warning());
        }

        let vendor = probe_request_with(&[221, 1, 7, 221, 1, 7]);
        let views = vendor.ie_explorer().elements().collect::<Vec<_>>();
        assert_eq!(views.len(), 2);
        for view in views {
            let warnings = view.warnings();
            let repetition = warnings.repetition().unwrap();
            assert!(!repetition.violates_singleton_cardinality());
            assert!(repetition.payloads_identical());
            assert!(!warnings.has_warning());
        }
    }

    #[test]
    fn ie_diff_covers_no_change_change_add_remove_and_wire_order() {
        let before = probe_request_with(&[0, 1, b'a', 3, 1, 11]);
        let identical = probe_request_with(&[0, 1, b'a', 3, 1, 11]);
        let no_change = diff_information_elements(&before, &identical).unwrap();
        assert!(no_change.is_empty());
        assert!(!no_change.subtype_changed());
        assert!(!no_change.wire_ie_sequence_changed());

        let changed = probe_request_with(&[0, 1, b'b', 3, 1, 11]);
        let diff = diff_information_elements(&before, &changed).unwrap();
        assert_eq!(diff.changes().len(), 1);
        assert_eq!(diff.changes()[0].kind(), ElementChangeKind::Modified);
        assert_eq!(diff.changes()[0].occurrence(), 0);
        assert_eq!(diff.changes()[0].before().unwrap().raw_payload(), b"a");
        assert_eq!(diff.changes()[0].after().unwrap().raw_payload(), b"b");
        assert!(diff.wire_ie_sequence_changed());

        let added = probe_request_with(&[0, 1, b'a', 3, 1, 11, 221, 1, 0x55]);
        let add_diff = diff_information_elements(&before, &added).unwrap();
        assert_eq!(add_diff.changes().len(), 1);
        assert_eq!(add_diff.changes()[0].kind(), ElementChangeKind::Added);
        assert_eq!(
            add_diff.changes()[0].after().unwrap().raw_bytes(),
            &[221, 1, 0x55]
        );

        let removed = diff_information_elements(&added, &before).unwrap();
        assert_eq!(removed.changes().len(), 1);
        assert_eq!(removed.changes()[0].kind(), ElementChangeKind::Removed);
        assert_eq!(removed.changes()[0].before().unwrap().index(), 2);

        let reordered = probe_request_with(&[3, 1, 11, 0, 1, b'a']);
        let order_diff = diff_information_elements(&before, &reordered).unwrap();
        assert!(order_diff.is_empty());
        assert!(order_diff.wire_ie_sequence_changed());
        assert_eq!(
            reordered
                .ie_explorer()
                .elements()
                .map(|element| element.standards_reference())
                .collect::<Vec<_>>(),
            [
                StandardsElementReference::Ieee80211ElementIdentifier {
                    id: 3,
                    extension_id: None
                },
                StandardsElementReference::Ieee80211ElementIdentifier {
                    id: 0,
                    extension_id: None
                }
            ]
        );
    }

    #[test]
    fn ie_diff_keeps_duplicate_occurrences_and_malformed_raw_evidence() {
        let before = probe_request_with(&[0, 1, b'a', 0, 1, b'b']);
        let after = probe_request_with(&[0, 1, b'a', 0, 1, b'c', 0, 1, b'd']);
        let diff = diff_information_elements(&before, &after).unwrap();
        assert_eq!(diff.changes().len(), 2);
        assert_eq!(diff.changes()[0].kind(), ElementChangeKind::Modified);
        assert_eq!(diff.changes()[0].occurrence(), 1);
        assert_eq!(
            diff.changes()[0].before().unwrap().raw_bytes(),
            &[0, 1, b'b']
        );
        assert_eq!(
            diff.changes()[0].after().unwrap().raw_bytes(),
            &[0, 1, b'c']
        );
        assert_eq!(diff.changes()[1].kind(), ElementChangeKind::Added);
        assert_eq!(diff.changes()[1].occurrence(), 2);
        assert_eq!(
            diff.changes()[1].after().unwrap().raw_bytes(),
            &[0, 1, b'd']
        );

        let malformed = probe_request_with(&[3, 2, 1, 2]);
        let malformed_changed = probe_request_with(&[3, 2, 1, 3]);
        let malformed_diff = diff_information_elements(&malformed, &malformed_changed).unwrap();
        assert_eq!(malformed_diff.changes().len(), 1);
        assert_eq!(
            malformed_diff.changes()[0]
                .before()
                .unwrap()
                .warnings()
                .malformed_problem(),
            Some(ElementProblem::DsLength)
        );
        assert_eq!(
            malformed_diff.changes()[0]
                .after()
                .unwrap()
                .warnings()
                .malformed_problem(),
            Some(ElementProblem::DsLength)
        );
    }

    #[test]
    fn ie_diff_exposes_frame_context_changes_even_when_raw_ies_match() {
        let probe = probe_request_with(&[0, 0]);
        let mut beacon_raw = header(8);
        beacon_raw.extend_from_slice(&[0; RESPONSE_FIXED_LEN]);
        beacon_raw.extend_from_slice(&[0, 0]);
        let beacon = parse(&beacon_raw, InputFraming::FcsAbsent).unwrap();
        assert_ne!(
            probe.elements()[0].decoded(),
            beacon.elements()[0].decoded()
        );
        let diff = diff_information_elements(&probe, &beacon).unwrap();
        assert!(diff.is_empty());
        assert!(diff.subtype_changed());
        assert!(!diff.wire_ie_sequence_changed());
    }

    #[test]
    fn ie_diff_obeys_work_allocation_payload_count_and_cancellation_limits() {
        struct Cancelled;
        impl Cancellation for Cancelled {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        let before = probe_request_with(&[0, 1, b'a']);
        let after = probe_request_with(&[0, 1, b'b']);
        let (_, usage) = diff_information_elements_with_usage(
            &before,
            &after,
            ParseLimits::default(),
            &NeverCancel,
        )
        .unwrap();
        let exact = ParseLimits {
            max_work_units: usage.work_units(),
            max_allocation_bytes: usage.allocation_bytes(),
            ..ParseLimits::default()
        };
        assert!(diff_information_elements_with_usage(&before, &after, exact, &NeverCancel).is_ok());
        assert!(matches!(
            diff_information_elements_with_usage(
                &before,
                &after,
                ParseLimits {
                    max_work_units: usage.work_units() - 1,
                    ..exact
                },
                &NeverCancel,
            ),
            Err(Error::LimitExceeded("work units"))
        ));
        assert!(matches!(
            diff_information_elements_with_usage(
                &before,
                &after,
                ParseLimits {
                    max_allocation_bytes: usage.allocation_bytes() - 1,
                    ..exact
                },
                &NeverCancel,
            ),
            Err(Error::LimitExceeded("allocation bytes"))
        ));
        assert!(matches!(
            diff_information_elements_with_usage(
                &before,
                &after,
                ParseLimits {
                    max_elements: 0,
                    ..ParseLimits::default()
                },
                &NeverCancel,
            ),
            Err(Error::LimitExceeded("element count"))
        ));
        assert!(matches!(
            diff_information_elements_with_usage(
                &before,
                &after,
                ParseLimits {
                    max_ie_payload_bytes: 0,
                    ..ParseLimits::default()
                },
                &NeverCancel,
            ),
            Err(Error::LimitExceeded("IE payload bytes"))
        ));
        assert!(matches!(
            diff_information_elements_with_usage(
                &before,
                &after,
                ParseLimits::default(),
                &Cancelled,
            ),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn ie_diff_accepts_default_maximum_element_count() {
        let mut raw = header(4);
        for _ in 0..ParseLimits::default().max_elements {
            raw.extend_from_slice(&[221, 0]);
        }
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        let (diff, usage) = diff_information_elements_with_usage(
            &frame,
            &frame,
            ParseLimits::default(),
            &NeverCancel,
        )
        .unwrap();
        assert!(diff.is_empty());
        assert!(!diff.wire_ie_sequence_changed());
        assert!(usage.work_units() <= ParseLimits::default().max_work_units);
        assert!(usage.allocation_bytes() <= ParseLimits::default().max_allocation_bytes);
    }

    #[test]
    fn ie_diff_heap_indexing_handles_mixed_order_and_polls_cancellation() {
        let mut reversed_ids = Vec::new();
        for id in (20..40_u8).rev() {
            reversed_ids.extend_from_slice(&[id, 0]);
        }
        let mixed = probe_request_with(&reversed_ids);
        let diff = diff_information_elements(&mixed, &mixed).unwrap();
        assert!(diff.is_empty());
        assert!(!diff.wire_ie_sequence_changed());

        struct CancelDuringSort(Cell<usize>);
        impl Cancellation for CancelDuringSort {
            fn is_cancelled(&self) -> bool {
                let check = self.0.get();
                self.0.set(check + 1);
                check >= 2
            }
        }
        let three = probe_request_with(&[22, 0, 20, 0, 21, 0]);
        assert!(matches!(
            diff_information_elements_with_usage(
                &three,
                &three,
                ParseLimits::default(),
                &CancelDuringSort(Cell::new(0)),
            ),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn singleton_duplicates_and_conflicts_are_explicit_but_vendor_repeats_are_allowed() {
        let mut raw = header(4);
        raw.extend_from_slice(&[0, 1, b'a', 0, 1, b'b', 221, 1, 1, 221, 1, 2]);
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        assert_eq!(frame.repeated_elements.len(), 2);
        assert!(frame.repeated_elements[0].violates_singleton_cardinality);
        assert!(frame.repeated_elements[0].contradictory);
        assert!(!frame.repeated_elements[1].violates_singleton_cardinality);
        assert!(!frame.repeated_elements[1].contradictory);
    }

    #[test]
    fn malformed_typed_elements_are_retained_while_broken_tlv_and_empty_extension_fail() {
        let mut raw = header(8);
        raw.extend_from_slice(&[0; 12]);
        raw.extend_from_slice(&[
            3, 2, 1, 2, 5, 4, 2, 2, 0, 1, 7, 7, b'U', b'S', b' ', 1, 2, 4, 9,
        ]);
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        assert!(matches!(
            frame.elements[0].decoded,
            ElementDecode::Malformed(ElementProblem::DsLength)
        ));
        assert!(matches!(
            frame.elements[1].decoded,
            ElementDecode::Malformed(ElementProblem::TimCountExceedsPeriod)
        ));
        assert!(matches!(
            frame.elements[2].decoded,
            ElementDecode::Malformed(ElementProblem::CountryInvalidPadding)
        ));
        let mut broken = header(4);
        broken.extend_from_slice(&[1, 8, 2]);
        assert!(matches!(
            parse(&broken, InputFraming::FcsAbsent),
            Err(Error::TruncatedInformationElement { .. })
        ));
        let mut extension = header(4);
        extension.extend_from_slice(&[255, 0]);
        assert!(matches!(
            parse(&extension, InputFraming::FcsAbsent),
            Err(Error::EmptyExtensionElement { .. })
        ));
    }

    #[test]
    fn limits_and_cancellation_stop_before_success() {
        let raw = beacon();
        let mut limits = ParseLimits {
            max_elements: 1,
            ..ParseLimits::default()
        };
        assert_eq!(
            parse_with(&raw, InputFraming::FcsAbsent, limits, &NeverCancel),
            Err(Error::LimitExceeded("element count"))
        );
        limits = ParseLimits {
            max_work_units: 1,
            ..ParseLimits::default()
        };
        assert_eq!(
            parse_with(&raw, InputFraming::FcsAbsent, limits, &NeverCancel),
            Err(Error::LimitExceeded("work units"))
        );
        struct CancelAfter(Cell<usize>);
        impl Cancellation for CancelAfter {
            fn is_cancelled(&self) -> bool {
                let n = self.0.get();
                self.0.set(n + 1);
                n >= 2
            }
        }
        assert_eq!(
            parse_with(
                &raw,
                InputFraming::FcsAbsent,
                ParseLimits::default(),
                &CancelAfter(Cell::new(0))
            ),
            Err(Error::Cancelled)
        );
    }

    #[test]
    fn canonical_bytes_are_golden_exact_and_revalidated() {
        let raw = header(4);
        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        let bytes = frame.canonical_bytes().unwrap();
        assert_eq!(
            bytes,
            [
                b"KY11IE\0".as_slice(),
                &[1, 0, 0, 24, 0, 0, 0],
                raw.as_slice()
            ]
            .concat()
        );
        assert_eq!(
            ManagementFrame::from_canonical_bytes(&bytes).unwrap(),
            frame
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            ManagementFrame::from_canonical_bytes(&trailing),
            Err(Error::CanonicalLength)
        );
        let mut tampered = bytes;
        tampered[9] = 2;
        assert_eq!(
            ManagementFrame::from_canonical_bytes(&tampered),
            Err(Error::CanonicalFraming(2))
        );
        let mut version = frame.canonical_bytes().unwrap();
        version[7] = 2;
        assert_eq!(
            ManagementFrame::from_canonical_bytes(&version),
            Err(Error::CanonicalVersion(2))
        );
    }

    #[test]
    fn element_count_boundary_is_exact() {
        let mut raw = header(4);
        for _ in 0..ParseLimits::default().max_elements {
            raw.extend_from_slice(&[221, 0]);
        }
        assert_eq!(
            parse(&raw, InputFraming::FcsAbsent).unwrap().elements.len(),
            1_024
        );
        raw.extend_from_slice(&[221, 0]);
        assert_eq!(
            parse(&raw, InputFraming::FcsAbsent),
            Err(Error::LimitExceeded("element count"))
        );
    }

    #[test]
    fn frame_and_payload_byte_boundaries_are_checked_before_success() {
        let raw = beacon();
        let frame_limit = ParseLimits {
            max_frame_bytes: raw.len() - 1,
            ..ParseLimits::default()
        };
        assert_eq!(
            parse_with(&raw, InputFraming::FcsAbsent, frame_limit, &NeverCancel),
            Err(Error::LimitExceeded("frame bytes"))
        );
        let payload_limit = ParseLimits {
            max_ie_payload_bytes: 1,
            ..ParseLimits::default()
        };
        assert_eq!(
            parse_with(&raw, InputFraming::FcsAbsent, payload_limit, &NeverCancel),
            Err(Error::LimitExceeded("IE payload bytes"))
        );
    }

    #[test]
    fn exact_header_and_fixed_field_truncations_are_rejected() {
        for len in 0..HEADER_LEN {
            assert_eq!(
                parse(&header(4)[..len], InputFraming::FcsAbsent),
                Err(Error::Truncated("management header"))
            );
        }
        let response = header(5);
        for added in 0..RESPONSE_FIXED_LEN {
            let mut raw = response.clone();
            raw.resize(HEADER_LEN + added, 0);
            assert_eq!(
                parse(&raw, InputFraming::FcsAbsent),
                Err(Error::Truncated("management fixed fields"))
            );
        }
    }

    #[test]
    fn unsupported_frame_classes_are_closed() {
        let mut raw = header(4);
        raw[0] |= 1;
        assert_eq!(
            parse(&raw, InputFraming::FcsAbsent),
            Err(Error::UnsupportedProtocolVersion(1))
        );
        raw[0] = (4 << 4) | (2 << 2);
        assert_eq!(
            parse(&raw, InputFraming::FcsAbsent),
            Err(Error::UnsupportedFrameType(2))
        );
        raw[0] = 11 << 4;
        assert_eq!(
            parse(&raw, InputFraming::FcsAbsent),
            Err(Error::UnsupportedManagementSubtype(11))
        );
    }

    #[test]
    fn crc_implementation_matches_the_standard_check_vector() {
        let mut control = Control {
            limits: ParseLimits::default(),
            cancellation: &NeverCancel,
            work: 0,
            allocation: 0,
        };
        assert_eq!(crc32(b"123456789", &mut control).unwrap(), 0xcbf4_3926);
    }

    #[test]
    fn canonical_encode_and_decode_poll_cancellation() {
        struct Cancelled;
        impl Cancellation for Cancelled {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        let frame = parse(&header(4), InputFraming::FcsAbsent).unwrap();
        assert_eq!(
            frame.canonical_bytes_with(ParseLimits::default(), &Cancelled),
            Err(Error::Cancelled)
        );
        let canonical = frame.canonical_bytes().unwrap();
        assert_eq!(
            ManagementFrame::from_canonical_bytes_with(
                &canonical,
                ParseLimits::default(),
                &Cancelled
            ),
            Err(Error::Cancelled)
        );
    }

    #[test]
    fn deterministic_3104_input_matrix_is_panic_free() {
        for len in 0..=96 {
            let mut bytes = vec![0_u8; len];
            for seed in 0..32_u8 {
                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte = seed.wrapping_mul(31).wrapping_add(index as u8);
                }
                let result = std::panic::catch_unwind(|| parse(&bytes, InputFraming::FcsAbsent));
                assert!(result.is_ok(), "len={len} seed={seed}");
            }
        }
    }

    #[test]
    fn ds_flags_are_rejected_for_every_supported_subtype_before_roles_publish() {
        for subtype in [4_u8, 5, 8] {
            for flags in [0x01_u8, 0x02, 0x03] {
                let mut raw = header(subtype);
                if subtype != 4 {
                    raw.extend_from_slice(&[0; RESPONSE_FIXED_LEN]);
                }
                raw[1] = flags;
                assert_eq!(
                    parse(&raw, InputFraming::FcsAbsent),
                    Err(Error::InvalidManagementDsFlags(flags))
                );
            }
            let mut raw = header(subtype);
            if subtype != 4 {
                raw.extend_from_slice(&[0; RESPONSE_FIXED_LEN]);
            }
            raw[1] = 0xfc; // all other management flag bits remain accepted
            let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
            assert_eq!(frame.addresses().receiver().octets(), [1, 2, 3, 4, 5, 6]);
            assert_eq!(frame.elements().len(), 0);
        }
    }

    #[test]
    fn country_legacy_even_length_and_padding_boundaries_preserve_raw() {
        for len in 5_usize..=12 {
            let mut payload = vec![1_u8; len];
            payload[..3].copy_from_slice(b"US ");
            if len == 10 {
                payload[9] = 0;
            }
            let mut raw = header(4);
            raw.extend_from_slice(&[7, len as u8]);
            raw.extend_from_slice(&payload);
            let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
            assert_eq!(frame.elements()[0].payload(), payload);
            let valid = matches!(len, 6 | 10 | 12);
            assert_eq!(
                matches!(frame.elements()[0].decoded(), ElementDecode::Country(_)),
                valid,
                "len={len}"
            );
        }
        let mut bad_pad = header(4);
        bad_pad.extend_from_slice(&[7, 10, b'U', b'S', b' ', 1, 2, 3, 4, 5, 6, 9]);
        let frame = parse(&bad_pad, InputFraming::FcsAbsent).unwrap();
        assert!(matches!(
            frame.elements()[0].decoded(),
            ElementDecode::Malformed(ElementProblem::CountryInvalidPadding)
        ));
    }

    #[test]
    fn tim_payload_boundaries_are_exact_and_raw_is_retained() {
        for len in [3_usize, 4, 254, 255] {
            let mut payload = vec![1_u8; len];
            if len >= 2 {
                payload[0] = 0;
                payload[1] = 1;
            }
            let mut raw = header(8);
            raw.extend_from_slice(&[0; RESPONSE_FIXED_LEN]);
            raw.extend_from_slice(&[5, len as u8]);
            raw.extend_from_slice(&payload);
            let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
            assert_eq!(frame.elements()[0].payload(), payload);
            assert_eq!(
                matches!(frame.elements()[0].decoded(), ElementDecode::Tim(_)),
                matches!(len, 4 | 254),
                "len={len}"
            );
        }
    }

    #[test]
    fn cumulative_parse_encode_decode_limits_are_exact() {
        let raw = beacon();
        let (frame, parse_usage) = parse_with_usage(
            &raw,
            InputFraming::FcsAbsent,
            ParseLimits::default(),
            &NeverCancel,
        )
        .unwrap();
        let exact = ParseLimits {
            max_work_units: parse_usage.work_units(),
            max_allocation_bytes: parse_usage.allocation_bytes(),
            ..ParseLimits::default()
        };
        assert!(parse_with(&raw, InputFraming::FcsAbsent, exact, &NeverCancel).is_ok());
        assert_eq!(
            parse_with(
                &raw,
                InputFraming::FcsAbsent,
                ParseLimits {
                    max_work_units: parse_usage.work_units() - 1,
                    ..exact
                },
                &NeverCancel
            ),
            Err(Error::LimitExceeded("work units"))
        );
        assert_eq!(
            parse_with(
                &raw,
                InputFraming::FcsAbsent,
                ParseLimits {
                    max_allocation_bytes: parse_usage.allocation_bytes() - 1,
                    ..exact
                },
                &NeverCancel
            ),
            Err(Error::LimitExceeded("allocation bytes"))
        );

        let (canonical, encode_usage) = frame
            .canonical_bytes_with_usage(ParseLimits::default(), &NeverCancel)
            .unwrap();
        let encode_exact = ParseLimits {
            max_work_units: encode_usage.work_units(),
            max_allocation_bytes: encode_usage.allocation_bytes(),
            ..ParseLimits::default()
        };
        assert!(
            frame
                .canonical_bytes_with(encode_exact, &NeverCancel)
                .is_ok()
        );
        assert_eq!(
            frame.canonical_bytes_with(
                ParseLimits {
                    max_work_units: encode_usage.work_units() - 1,
                    ..encode_exact
                },
                &NeverCancel
            ),
            Err(Error::LimitExceeded("work units"))
        );
        assert_eq!(
            frame.canonical_bytes_with(
                ParseLimits {
                    max_allocation_bytes: encode_usage.allocation_bytes() - 1,
                    ..encode_exact
                },
                &NeverCancel
            ),
            Err(Error::LimitExceeded("allocation bytes"))
        );

        let (_, decode_usage) = ManagementFrame::from_canonical_bytes_with_usage(
            &canonical,
            ParseLimits::default(),
            &NeverCancel,
        )
        .unwrap();
        let decode_exact = ParseLimits {
            max_work_units: decode_usage.work_units(),
            max_allocation_bytes: decode_usage.allocation_bytes(),
            ..ParseLimits::default()
        };
        assert!(
            ManagementFrame::from_canonical_bytes_with(&canonical, decode_exact, &NeverCancel)
                .is_ok()
        );
        assert_eq!(
            ManagementFrame::from_canonical_bytes_with(
                &canonical,
                ParseLimits {
                    max_work_units: decode_usage.work_units() - 1,
                    ..decode_exact
                },
                &NeverCancel
            ),
            Err(Error::LimitExceeded("work units"))
        );
        assert_eq!(
            ManagementFrame::from_canonical_bytes_with(
                &canonical,
                ParseLimits {
                    max_allocation_bytes: decode_usage.allocation_bytes() - 1,
                    ..decode_exact
                },
                &NeverCancel
            ),
            Err(Error::LimitExceeded("allocation bytes"))
        );
    }

    #[test]
    fn maximum_frame_payload_and_element_scratch_are_admitted_then_bounded() {
        let mut raw = header(4);
        for value in 0..44_u8 {
            raw.extend_from_slice(&[221, 255]);
            raw.extend(std::iter::repeat_n(value, 255));
        }
        raw.extend_from_slice(&[221, 120]);
        raw.extend(std::iter::repeat_n(0xaa, 120));
        assert_eq!(raw.len(), ParseLimits::default().max_frame_bytes);
        let (frame, usage) = parse_with_usage(
            &raw,
            InputFraming::FcsAbsent,
            ParseLimits::default(),
            &NeverCancel,
        )
        .unwrap();
        assert_eq!(frame.elements().len(), 45);
        assert_eq!(
            frame
                .elements()
                .iter()
                .map(|ie| ie.payload().len())
                .sum::<usize>(),
            11_340
        );
        assert!(usage.allocation_bytes() <= ParseLimits::default().max_allocation_bytes);
        let limits = ParseLimits {
            max_allocation_bytes: usage.allocation_bytes() - 1,
            ..ParseLimits::default()
        };
        assert_eq!(
            parse_with(&raw, InputFraming::FcsAbsent, limits, &NeverCancel),
            Err(Error::LimitExceeded("allocation bytes"))
        );
    }

    #[test]
    fn cancellation_is_live_in_crc_tlv_grouping_copy_and_replay_phases() {
        struct CancelAt {
            calls: Cell<usize>,
            at: usize,
        }
        impl Cancellation for CancelAt {
            fn is_cancelled(&self) -> bool {
                let call = self.calls.get();
                self.calls.set(call + 1);
                call >= self.at
            }
        }
        let cancel = |at| CancelAt {
            calls: Cell::new(0),
            at,
        };
        let limits = ParseLimits::default();

        let mut control = Control {
            limits,
            cancellation: &cancel(1),
            work: 0,
            allocation: 0,
        };
        assert_eq!(crc32(&[0; 128], &mut control), Err(Error::Cancelled));

        let mut raw = header(4);
        raw.extend_from_slice(&[221, 0, 221, 0]);
        let mut control = Control {
            limits,
            cancellation: &cancel(1),
            work: 0,
            allocation: 0,
        };
        assert_eq!(
            preflight_elements(&raw, HEADER_LEN, raw.len(), &mut control),
            Err(Error::Cancelled)
        );

        let frame = parse(&raw, InputFraming::FcsAbsent).unwrap();
        let mut control = Control {
            limits,
            cancellation: &cancel(1),
            work: 0,
            allocation: 0,
        };
        assert_eq!(
            summarize_repeats(frame.elements(), &mut control),
            Err(Error::Cancelled)
        );

        assert_eq!(
            frame.canonical_bytes_with(limits, &cancel(2)),
            Err(Error::Cancelled)
        );
        let canonical = frame.canonical_bytes().unwrap();
        assert_eq!(
            ManagementFrame::from_canonical_bytes_with(&canonical, limits, &cancel(2)),
            Err(Error::Cancelled)
        );
    }
}
