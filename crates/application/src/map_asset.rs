//! Bounded, content-derived admission for the initial raster subset.
//!
//! Admission deliberately does not decode pixels or validate the IDAT zlib/
//! DEFLATE stream. The initial subset is PNG because its container can be
//! walked, length-bounded and CRC-verified without expanding attacker data.

use crate::{ApplicationError, ErrorKind, error::map_budget_error};
use kyberia_domain::identity::ContentHash;
use kyberia_resource_budget::{BudgetKind, CancellationHook, ResourceBudget};
use std::num::NonZeroU32;

pub const PNG_MEDIA_TYPE: &str = "image/png";
pub const MAX_MAP_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_MAP_DIMENSION: u32 = 32_768;
pub const MAX_MAP_PIXELS: u64 = 100_000_000;
pub const MAX_PNG_CHUNKS: usize = 4_096;
pub const MAX_PNG_METADATA_BYTES: usize = 4 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Container-derived PNG metadata and source hash only; this is not proof that
/// IDAT can be decompressed or that the pixels are displayable.
pub struct AdmittedMapAsset {
    width: NonZeroU32,
    height: NonZeroU32,
    content_hash: ContentHash,
}

impl AdmittedMapAsset {
    pub const fn width(self) -> NonZeroU32 {
        self.width
    }
    pub const fn height(self) -> NonZeroU32 {
        self.height
    }
    pub const fn content_hash(self) -> ContentHash {
        self.content_hash
    }
    pub const fn media_type(self) -> &'static str {
        PNG_MEDIA_TYPE
    }
}

fn invalid(message: &'static str) -> ApplicationError {
    ApplicationError::new(ErrorKind::InvalidRequest, message)
}

pub fn validate_map_provenance(value: &str) -> Result<(), ApplicationError> {
    if value.trim().is_empty()
        || value.len() > 1024
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-' | b'.'))
    {
        return Err(invalid("map provenance must be an opaque identifier"));
    }
    Ok(())
}

/// Validate a strict PNG subset from bytes. Filename extensions and caller
/// MIME hints are intentionally absent from this boundary and cannot affect
/// the derived media type.
pub fn admit_map_asset<H: CancellationHook>(
    bytes: &[u8],
    budget: &mut ResourceBudget<H>,
) -> Result<AdmittedMapAsset, ApplicationError> {
    budget.check_cancelled().map_err(map_budget_error)?;
    if bytes.len() > MAX_MAP_SOURCE_BYTES {
        return Err(ApplicationError::new(
            ErrorKind::ResourceLimit,
            "map source bytes",
        ));
    }
    budget
        .charge(BudgetKind::OperationBytes, bytes.len())
        .map_err(map_budget_error)?;
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(invalid("unsupported raster content"));
    }

    let mut offset = PNG_SIGNATURE.len();
    let mut chunks = 0usize;
    let mut dimensions = None;
    let mut saw_palette = false;
    let mut palette_entries = 0usize;
    let mut saw_transparency = false;
    let mut saw_idat = false;
    let mut ended_idat = false;
    let mut metadata_bytes = 0usize;
    let mut saw_srgb = false;
    let mut saw_gamma = false;
    let mut saw_phys = false;
    while offset < bytes.len() {
        budget.check_cancelled().map_err(map_budget_error)?;
        chunks = chunks
            .checked_add(1)
            .ok_or_else(|| invalid("PNG chunk count overflow"))?;
        if chunks > MAX_PNG_CHUNKS {
            return Err(ApplicationError::new(
                ErrorKind::ResourceLimit,
                "PNG chunk count",
            ));
        }
        budget
            .charge(BudgetKind::OperationAncestryWork, 1)
            .map_err(map_budget_error)?;
        let header_end = offset
            .checked_add(8)
            .ok_or_else(|| invalid("PNG length overflow"))?;
        if header_end > bytes.len() {
            return Err(invalid("truncated PNG chunk header"));
        }
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[offset + 4..header_end].try_into().unwrap();
        if !kind.iter().all(u8::is_ascii_alphabetic) || kind[2].is_ascii_lowercase() {
            return Err(invalid("invalid PNG chunk type"));
        }
        let data_end = header_end
            .checked_add(length)
            .ok_or_else(|| invalid("PNG chunk length overflow"))?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or_else(|| invalid("PNG chunk length overflow"))?;
        if chunk_end > bytes.len() {
            return Err(invalid("truncated PNG chunk"));
        }
        let expected_crc = u32::from_be_bytes(bytes[data_end..chunk_end].try_into().unwrap());
        if crc32(&bytes[offset + 4..data_end]) != expected_crc {
            return Err(invalid("PNG chunk CRC mismatch"));
        }
        let data = &bytes[header_end..data_end];
        if !matches!(&kind, b"IHDR" | b"IDAT" | b"IEND") {
            metadata_bytes = metadata_bytes
                .checked_add(length)
                .ok_or_else(|| invalid("PNG metadata length overflow"))?;
            if metadata_bytes > MAX_PNG_METADATA_BYTES {
                return Err(ApplicationError::new(
                    ErrorKind::ResourceLimit,
                    "PNG metadata bytes",
                ));
            }
        }
        match &kind {
            b"IHDR" => {
                if chunks != 1 || length != 13 || dimensions.is_some() {
                    return Err(invalid("invalid PNG IHDR"));
                }
                let width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                let width = NonZeroU32::new(width).ok_or_else(|| invalid("zero PNG width"))?;
                let height = NonZeroU32::new(height).ok_or_else(|| invalid("zero PNG height"))?;
                if width.get() > MAX_MAP_DIMENSION || height.get() > MAX_MAP_DIMENSION {
                    return Err(ApplicationError::new(
                        ErrorKind::ResourceLimit,
                        "PNG dimensions",
                    ));
                }
                let pixels = u64::from(width.get()) * u64::from(height.get());
                if pixels > MAX_MAP_PIXELS {
                    return Err(ApplicationError::new(
                        ErrorKind::ResourceLimit,
                        "PNG pixel count",
                    ));
                }
                validate_ihdr(data)?;
                dimensions = Some((width, height, data[8], data[9]));
            }
            b"PLTE" => {
                let color_type = dimensions.map(|value| value.3);
                if !matches!(color_type, Some(2 | 3 | 6))
                    || saw_palette
                    || saw_idat
                    || length == 0
                    || length > 768
                    || !length.is_multiple_of(3)
                {
                    return Err(invalid("invalid PNG palette"));
                }
                if color_type == Some(3) {
                    let bit_depth = dimensions.expect("indexed palette has IHDR").2;
                    let maximum_entries = 1usize << bit_depth;
                    if length / 3 > maximum_entries {
                        return Err(invalid("PNG palette exceeds indexed bit depth"));
                    }
                }
                saw_palette = true;
                palette_entries = length / 3;
            }
            b"tRNS" => {
                let Some((_, _, _, color_type)) = dimensions else {
                    return Err(invalid("PNG metadata precedes IHDR"));
                };
                let valid = !saw_idat
                    && !saw_transparency
                    && match color_type {
                        0 => length == 2,
                        2 => length == 6,
                        3 => saw_palette && length > 0 && length <= palette_entries,
                        _ => false,
                    };
                if !valid {
                    return Err(invalid("invalid PNG transparency"));
                }
                saw_transparency = true;
            }
            b"IDAT" => {
                if dimensions.is_none() || ended_idat || length == 0 {
                    return Err(invalid("invalid PNG image data order"));
                }
                saw_idat = true;
            }
            b"IEND" => {
                if length != 0 || !saw_idat || dimensions.is_none() || chunk_end != bytes.len() {
                    return Err(invalid("invalid PNG end or trailing content"));
                }
                let (width, height, _, color_type) = dimensions.unwrap();
                if color_type == 3 && !saw_palette {
                    return Err(invalid("indexed PNG has no palette"));
                }
                return Ok(AdmittedMapAsset {
                    width,
                    height,
                    content_hash: ContentHash::try_from(kyberia_project_store::content_hash(bytes))
                        .expect("project-store SHA-256 is canonical"),
                });
            }
            // A deliberately narrow metadata subset. Text, profiles, EXIF,
            // animation and unknown ancillary payloads are rejected so an
            // admitted artifact cannot smuggle an ambiguous second document.
            b"sRGB"
                if dimensions.is_some()
                    && !saw_idat
                    && !saw_srgb
                    && length == 1
                    && data[0] <= 3 =>
            {
                saw_srgb = true;
            }
            b"gAMA"
                if dimensions.is_some()
                    && !saw_idat
                    && !saw_gamma
                    && length == 4
                    && data != [0; 4] =>
            {
                saw_gamma = true;
            }
            b"pHYs"
                if dimensions.is_some()
                    && !saw_idat
                    && !saw_phys
                    && length == 9
                    && data[8] <= 1 =>
            {
                saw_phys = true;
            }
            _ => return Err(invalid("unsupported PNG chunk")),
        }
        if saw_idat && kind != *b"IDAT" {
            ended_idat = true;
        }
        offset = chunk_end;
    }
    Err(invalid("PNG is missing IEND"))
}

/// Compatibility boundary for file pickers that possess untrusted filename
/// and MIME hints. Hints are deliberately neither parsed nor retained: only
/// content determines admission and the canonical media type.
pub fn admit_map_asset_with_hints<H: CancellationHook>(
    bytes: &[u8],
    _file_extension: Option<&str>,
    _declared_media_type: Option<&str>,
    budget: &mut ResourceBudget<H>,
) -> Result<AdmittedMapAsset, ApplicationError> {
    admit_map_asset(bytes, budget)
}

fn validate_ihdr(data: &[u8]) -> Result<(), ApplicationError> {
    let bit_depth = data[8];
    let color_type = data[9];
    let valid_depth = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        2 => matches!(bit_depth, 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        4 | 6 => matches!(bit_depth, 8 | 16),
        _ => false,
    };
    if !valid_depth || data[10] != 0 || data[11] != 0 || data[12] > 1 {
        return Err(invalid("unsupported PNG IHDR encoding"));
    }
    Ok(())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_resource_budget::{ResourceBudget, ResourceLimits};

    fn budget() -> ResourceBudget {
        ResourceBudget::new(ResourceLimits::new(
            10_000,
            10,
            10,
            64 * 1024 * 1024,
            10,
            10,
        ))
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&out[4..]).to_be_bytes());
        out
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        out.extend(chunk(b"IHDR", &ihdr));
        // Deliberately truncated zlib data: admission verifies the container
        // and hashes bytes, but does not validate or inflate the pixel stream.
        out.extend(chunk(b"IDAT", &[0x78, 0x01, 0x01]));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn admits_png_container_without_validating_the_pixel_stream() {
        let bytes = png(16, 9);
        let admitted = admit_map_asset(&bytes, &mut budget()).unwrap();
        assert_eq!(admitted.width().get(), 16);
        assert_eq!(admitted.height().get(), 9);
        assert_eq!(admitted.media_type(), PNG_MEDIA_TYPE);
        let mismatched =
            admit_map_asset_with_hints(&bytes, Some("jpg"), Some("image/jpeg"), &mut budget())
                .unwrap();
        assert_eq!(mismatched, admitted);
    }

    #[test]
    fn rejects_truncation_bad_crc_and_trailing_polyglot() {
        let bytes = png(1, 1);
        assert_eq!(
            admit_map_asset(&bytes[..bytes.len() - 1], &mut budget())
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidRequest
        );
        let mut bad_crc = bytes.clone();
        bad_crc[29] ^= 1;
        assert_eq!(
            admit_map_asset(&bad_crc, &mut budget()).unwrap_err().kind(),
            ErrorKind::InvalidRequest
        );
        let mut trailing = bytes;
        trailing.extend_from_slice(b"PK\x03\x04");
        assert_eq!(
            admit_map_asset(&trailing, &mut budget())
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidRequest
        );
    }

    #[test]
    fn rejects_zero_extreme_and_excess_pixel_declarations() {
        for (width, height) in [(0, 1), (MAX_MAP_DIMENSION + 1, 1), (20_000, 20_000)] {
            assert!(admit_map_asset(&png(width, height), &mut budget()).is_err());
        }
    }

    #[test]
    fn rejects_malformed_lengths_and_unsupported_content() {
        let mut malformed = png(1, 1);
        malformed[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            admit_map_asset(&malformed, &mut budget())
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidRequest
        );
        assert_eq!(
            admit_map_asset(b"pretend.jpg", &mut budget())
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidRequest
        );
    }

    #[test]
    fn rejects_unknown_or_duplicate_metadata_and_chunk_work_exhaustion() {
        let canonical = png(1, 1);
        let ihdr_end = 8 + 25;
        let mut unknown = canonical[..ihdr_end].to_vec();
        unknown.extend(chunk(b"tEXt", b"path\0/private/secret"));
        unknown.extend_from_slice(&canonical[ihdr_end..]);
        assert_eq!(
            admit_map_asset(&unknown, &mut budget()).unwrap_err().kind(),
            ErrorKind::InvalidRequest
        );

        let mut tiny = ResourceBudget::new(ResourceLimits::new(1, 1, 1, 1_000, 1, 1));
        assert_eq!(
            admit_map_asset(&canonical, &mut tiny).unwrap_err().kind(),
            ErrorKind::ResourceLimit
        );
    }

    #[test]
    fn rejects_indexed_palette_larger_than_declared_bit_depth() {
        let mut bytes = PNG_SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&[1, 3, 0, 0, 0]);
        bytes.extend(chunk(b"IHDR", &ihdr));
        // A one-bit indexed image can address only two palette entries.
        bytes.extend(chunk(b"PLTE", &[0, 0, 0, 255, 255, 255, 1, 2, 3]));
        bytes.extend(chunk(b"IDAT", &[0x78, 0x01, 0x01]));
        bytes.extend(chunk(b"IEND", &[]));

        assert_eq!(
            admit_map_asset(&bytes, &mut budget()).unwrap_err().kind(),
            ErrorKind::InvalidRequest
        );
    }
}
