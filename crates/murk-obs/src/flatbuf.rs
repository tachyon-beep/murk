//! Binary serialization for observation specs.
//!
//! Provides round-trip serialization of [`ObsSpec`] using a compact
//! binary format. The format uses a "MOBS" file identifier and
//! version 1.
//!
//! Wire format:
//! ```text
//! [4 bytes] magic "MOBS"
//! [2 bytes] version (little-endian u16)
//! [2 bytes] n_entries (little-endian u16)
//! [n_entries × entry]
//! ```
//!
//! Each entry:
//! ```text
//! [4 bytes] field_id (LE u32)
//! [1 byte]  region_type
//! [2 bytes] n_region_params (LE u16)
//! [n × 4 bytes] region_params (LE i32 each)
//! [1 byte]  transform_type
//! [8 bytes] normalize_min (LE f64, if Normalize)
//! [8 bytes] normalize_max (LE f64, if Normalize)
//! [1 byte]  dtype
//! [1 byte]  pool_kernel (0=None)
//! [4 bytes] pool_kernel_size (LE u32, if pool_kernel != 0)
//! [4 bytes] pool_stride (LE u32, if pool_kernel != 0)
//! ```

use crate::spec::{ObsDtype, ObsEntry, ObsRegion, ObsSpec, ObsTransform, PoolConfig, PoolKernel};
use murk_core::error::ObsError;
use murk_core::FieldId;
use murk_space::RegionSpec;
use smallvec::SmallVec;

const MAGIC: &[u8; 4] = b"MOBS";
const VERSION: u16 = 1;

// Region type tags
const REGION_ALL: u8 = 0;
const REGION_DISK: u8 = 1;
const REGION_RECT: u8 = 2;
const REGION_NEIGHBOURS: u8 = 3;
const REGION_COORDS: u8 = 4;
const REGION_AGENT_DISK: u8 = 5;
const REGION_AGENT_RECT: u8 = 6;

// Transform type tags
const TRANSFORM_IDENTITY: u8 = 0;
const TRANSFORM_NORMALIZE: u8 = 1;

// Pool kernel tags
const POOL_NONE: u8 = 0;
const POOL_MEAN: u8 = 1;
const POOL_MAX: u8 = 2;
const POOL_MIN: u8 = 3;
const POOL_SUM: u8 = 4;

/// Serialize an [`ObsSpec`] to binary bytes.
///
/// Returns `Err` if any value exceeds its wire-format range
/// (e.g. more than `u16::MAX` entries).
pub fn serialize(spec: &ObsSpec) -> Result<Vec<u8>, ObsError> {
    let mut buf = Vec::with_capacity(128);

    // Header
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    let n_entries = u16::try_from(spec.entries.len()).map_err(|_| ObsError::InvalidObsSpec {
        reason: format!(
            "too many entries: {} exceeds u16::MAX ({})",
            spec.entries.len(),
            u16::MAX
        ),
    })?;
    buf.extend_from_slice(&n_entries.to_le_bytes());

    for entry in &spec.entries {
        write_entry(&mut buf, entry)?;
    }

    Ok(buf)
}

fn write_entry(buf: &mut Vec<u8>, entry: &ObsEntry) -> Result<(), ObsError> {
    buf.extend_from_slice(&entry.field_id.0.to_le_bytes());

    // Region
    let (region_tag, region_params) = encode_region(&entry.region)?;
    buf.push(region_tag);
    let n_params = u16::try_from(region_params.len()).map_err(|_| ObsError::InvalidObsSpec {
        reason: format!(
            "too many region params: {} exceeds u16::MAX",
            region_params.len()
        ),
    })?;
    buf.extend_from_slice(&n_params.to_le_bytes());
    for &p in &region_params {
        buf.extend_from_slice(&p.to_le_bytes());
    }

    // Transform
    match &entry.transform {
        ObsTransform::Identity => {
            buf.push(TRANSFORM_IDENTITY);
        }
        ObsTransform::Normalize { min, max } => {
            buf.push(TRANSFORM_NORMALIZE);
            buf.extend_from_slice(&min.to_le_bytes());
            buf.extend_from_slice(&max.to_le_bytes());
        }
    }

    // Dtype
    match entry.dtype {
        ObsDtype::F32 => buf.push(0),
    }

    // Pool
    match &entry.pool {
        None => buf.push(POOL_NONE),
        Some(cfg) => {
            let tag = match cfg.kernel {
                PoolKernel::Mean => POOL_MEAN,
                PoolKernel::Max => POOL_MAX,
                PoolKernel::Min => POOL_MIN,
                PoolKernel::Sum => POOL_SUM,
            };
            buf.push(tag);
            let ks = u32::try_from(cfg.kernel_size).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("kernel_size {} exceeds u32::MAX", cfg.kernel_size),
            })?;
            let st = u32::try_from(cfg.stride).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("stride {} exceeds u32::MAX", cfg.stride),
            })?;
            buf.extend_from_slice(&ks.to_le_bytes());
            buf.extend_from_slice(&st.to_le_bytes());
        }
    }

    Ok(())
}

fn encode_region(region: &ObsRegion) -> Result<(u8, Vec<i32>), ObsError> {
    match region {
        ObsRegion::Fixed(RegionSpec::All) => Ok((REGION_ALL, vec![])),
        ObsRegion::Fixed(RegionSpec::Disk { center, radius }) => {
            let mut params: Vec<i32> = center.iter().copied().collect();
            let r = i32::try_from(*radius).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("Disk radius {radius} exceeds i32::MAX"),
            })?;
            params.push(r);
            Ok((REGION_DISK, params))
        }
        ObsRegion::Fixed(RegionSpec::Rect { min, max }) => {
            let mut params: Vec<i32> = min.iter().copied().collect();
            params.extend(max.iter().copied());
            Ok((REGION_RECT, params))
        }
        ObsRegion::Fixed(RegionSpec::Neighbours { center, depth }) => {
            let mut params: Vec<i32> = center.iter().copied().collect();
            let d = i32::try_from(*depth).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("Neighbours depth {depth} exceeds i32::MAX"),
            })?;
            params.push(d);
            Ok((REGION_NEIGHBOURS, params))
        }
        ObsRegion::Fixed(RegionSpec::Coords(coords)) => {
            let ndim = coords.first().map(|c| c.len()).unwrap_or(0) as i32;
            let n_coords = coords.len() as i32;
            let mut params = vec![ndim, n_coords];
            for c in coords {
                params.extend(c.iter().copied());
            }
            Ok((REGION_COORDS, params))
        }
        ObsRegion::AgentDisk { radius } => {
            let r = i32::try_from(*radius).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("AgentDisk radius {radius} exceeds i32::MAX"),
            })?;
            Ok((REGION_AGENT_DISK, vec![r]))
        }
        ObsRegion::AgentRect { half_extent } => {
            let params: Vec<i32> = half_extent
                .iter()
                .map(|&h| {
                    i32::try_from(h).map_err(|_| ObsError::InvalidObsSpec {
                        reason: format!("AgentRect half_extent {h} exceeds i32::MAX"),
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok((REGION_AGENT_RECT, params))
        }
    }
}

/// Deserialize an [`ObsSpec`] from binary bytes.
pub fn deserialize(bytes: &[u8]) -> Result<ObsSpec, ObsError> {
    let mut r = Reader::new(bytes);

    // Magic
    let magic = r.read_bytes(4)?;
    if magic != MAGIC {
        return Err(ObsError::InvalidObsSpec {
            reason: format!(
                "invalid magic: expected 'MOBS', got '{}'",
                String::from_utf8_lossy(magic)
            ),
        });
    }

    // Version
    let version = r.read_u16()?;
    if version > VERSION {
        return Err(ObsError::InvalidObsSpec {
            reason: format!("unsupported version {version}, max supported is {VERSION}"),
        });
    }

    // Entries
    let n_entries = r.read_u16()? as usize;
    let mut entries = Vec::with_capacity(n_entries);
    for i in 0..n_entries {
        entries.push(read_entry(&mut r, i)?);
    }

    if r.pos != bytes.len() {
        return Err(ObsError::InvalidObsSpec {
            reason: format!(
                "trailing bytes: {} unconsumed after {} entries",
                bytes.len() - r.pos,
                n_entries
            ),
        });
    }

    Ok(ObsSpec { entries })
}

fn read_entry(r: &mut Reader<'_>, idx: usize) -> Result<ObsEntry, ObsError> {
    let field_id = FieldId(r.read_u32().map_err(|e| truncated(idx, &e))?);

    // Region
    let region_tag = r.read_u8().map_err(|e| truncated(idx, &e))?;
    let n_params = r.read_u16().map_err(|e| truncated(idx, &e))? as usize;
    let mut region_params = Vec::with_capacity(n_params);
    for _ in 0..n_params {
        region_params.push(r.read_i32().map_err(|e| truncated(idx, &e))?);
    }
    let region = decode_region(region_tag, &region_params, idx)?;

    // Transform
    let transform_tag = r.read_u8().map_err(|e| truncated(idx, &e))?;
    let transform = match transform_tag {
        TRANSFORM_IDENTITY => ObsTransform::Identity,
        TRANSFORM_NORMALIZE => {
            let min = r.read_f64().map_err(|e| truncated(idx, &e))?;
            let max = r.read_f64().map_err(|e| truncated(idx, &e))?;
            ObsTransform::Normalize { min, max }
        }
        other => {
            return Err(ObsError::InvalidObsSpec {
                reason: format!("entry {idx}: unknown transform type {other}"),
            });
        }
    };

    // Dtype
    let dtype_val = r.read_u8().map_err(|e| truncated(idx, &e))?;
    let dtype = match dtype_val {
        0 => ObsDtype::F32,
        other => {
            return Err(ObsError::InvalidObsSpec {
                reason: format!("entry {idx}: unknown dtype {other}"),
            });
        }
    };

    // Pool
    let pool_tag = r.read_u8().map_err(|e| truncated(idx, &e))?;
    let pool = if pool_tag == POOL_NONE {
        None
    } else {
        let kernel = match pool_tag {
            POOL_MEAN => PoolKernel::Mean,
            POOL_MAX => PoolKernel::Max,
            POOL_MIN => PoolKernel::Min,
            POOL_SUM => PoolKernel::Sum,
            other => {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: unknown pool kernel {other}"),
                });
            }
        };
        let kernel_size = r.read_u32().map_err(|e| truncated(idx, &e))? as usize;
        let stride = r.read_u32().map_err(|e| truncated(idx, &e))? as usize;
        if kernel_size == 0 || stride == 0 {
            return Err(ObsError::InvalidObsSpec {
                reason: format!("entry {idx}: pool kernel_size and stride must be > 0"),
            });
        }
        Some(PoolConfig {
            kernel,
            kernel_size,
            stride,
        })
    };

    Ok(ObsEntry {
        field_id,
        region,
        pool,
        transform,
        dtype,
    })
}

fn decode_region(tag: u8, params: &[i32], idx: usize) -> Result<ObsRegion, ObsError> {
    match tag {
        REGION_ALL => Ok(ObsRegion::Fixed(RegionSpec::All)),
        REGION_DISK => {
            if params.len() < 2 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: Disk region needs at least 2 params"),
                });
            }
            let ndim = params.len() - 1;
            let center: SmallVec<[i32; 4]> = params[..ndim].iter().copied().collect();
            let radius = u32::try_from(params[ndim]).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("entry {idx}: negative Disk radius {}", params[ndim]),
            })?;
            Ok(ObsRegion::Fixed(RegionSpec::Disk { center, radius }))
        }
        REGION_RECT => {
            if params.is_empty() || params.len() % 2 != 0 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: Rect region needs even number of params"),
                });
            }
            let ndim = params.len() / 2;
            let min: SmallVec<[i32; 4]> = params[..ndim].iter().copied().collect();
            let max: SmallVec<[i32; 4]> = params[ndim..].iter().copied().collect();
            Ok(ObsRegion::Fixed(RegionSpec::Rect { min, max }))
        }
        REGION_NEIGHBOURS => {
            if params.len() < 2 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: Neighbours region needs at least 2 params"),
                });
            }
            let ndim = params.len() - 1;
            let center: SmallVec<[i32; 4]> = params[..ndim].iter().copied().collect();
            let depth = u32::try_from(params[ndim]).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("entry {idx}: negative Neighbours depth {}", params[ndim]),
            })?;
            Ok(ObsRegion::Fixed(RegionSpec::Neighbours { center, depth }))
        }
        REGION_COORDS => {
            if params.len() < 2 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: Coords region needs ndim + n_coords header"),
                });
            }
            let ndim = params[0] as usize;
            let n_coords = params[1] as usize;
            let data = &params[2..];
            if ndim == 0 || data.len() != ndim * n_coords {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!(
                        "entry {idx}: Coords expected {} values, got {}",
                        ndim * n_coords,
                        data.len()
                    ),
                });
            }
            let coords: Vec<SmallVec<[i32; 4]>> = data
                .chunks(ndim)
                .map(|chunk| chunk.iter().copied().collect())
                .collect();
            Ok(ObsRegion::Fixed(RegionSpec::Coords(coords)))
        }
        REGION_AGENT_DISK => {
            if params.len() != 1 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: AgentDisk needs exactly 1 param (radius)"),
                });
            }
            let radius = u32::try_from(params[0]).map_err(|_| ObsError::InvalidObsSpec {
                reason: format!("entry {idx}: negative AgentDisk radius {}", params[0]),
            })?;
            Ok(ObsRegion::AgentDisk { radius })
        }
        REGION_AGENT_RECT => {
            if params.is_empty() {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {idx}: AgentRect needs at least 1 param"),
                });
            }
            let half_extent: SmallVec<[u32; 4]> = params
                .iter()
                .map(|&p| {
                    u32::try_from(p).map_err(|_| ObsError::InvalidObsSpec {
                        reason: format!("entry {idx}: negative AgentRect half_extent {p}"),
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok(ObsRegion::AgentRect { half_extent })
        }
        other => Err(ObsError::InvalidObsSpec {
            reason: format!("entry {idx}: unknown region type {other}"),
        }),
    }
}

fn truncated(idx: usize, _inner: &ObsError) -> ObsError {
    ObsError::InvalidObsSpec {
        reason: format!("entry {idx}: truncated data"),
    }
}

/// Simple cursor reader for safe byte parsing.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], ObsError> {
        if self.pos + n > self.data.len() {
            return Err(ObsError::InvalidObsSpec {
                reason: "unexpected end of data".into(),
            });
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, ObsError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, ObsError> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, ObsError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_i32(&mut self) -> Result<i32, ObsError> {
        let b = self.read_bytes(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_f64(&mut self) -> Result<f64, ObsError> {
        let b = self.read_bytes(8)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    fn round_trip(spec: &ObsSpec) -> ObsSpec {
        let bytes = serialize(spec).unwrap();
        deserialize(&bytes).unwrap()
    }

    #[test]
    fn round_trip_all_identity() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_normalize() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(7),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Normalize {
                    min: -1.0,
                    max: 5.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_disk_region() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Disk {
                    center: smallvec![3, 4],
                    radius: 5,
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_rect_region() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Rect {
                    min: smallvec![1, 2],
                    max: smallvec![5, 8],
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_neighbours_region() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Neighbours {
                    center: smallvec![3, 4],
                    depth: 2,
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_coords_region() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Coords(vec![
                    smallvec![0, 0],
                    smallvec![1, 2],
                    smallvec![3, 4],
                ])),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_agent_disk() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk { radius: 3 },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_agent_rect() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec![3, 4],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_with_pool() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(2),
                region: ObsRegion::AgentDisk { radius: 5 },
                pool: Some(PoolConfig {
                    kernel: PoolKernel::Mean,
                    kernel_size: 2,
                    stride: 2,
                }),
                transform: ObsTransform::Normalize {
                    min: 0.0,
                    max: 100.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_multiple_entries() {
        let spec = ObsSpec {
            entries: vec![
                ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                ObsEntry {
                    field_id: FieldId(1),
                    region: ObsRegion::AgentDisk { radius: 3 },
                    pool: Some(PoolConfig {
                        kernel: PoolKernel::Max,
                        kernel_size: 3,
                        stride: 1,
                    }),
                    transform: ObsTransform::Normalize {
                        min: -5.0,
                        max: 5.0,
                    },
                    dtype: ObsDtype::F32,
                },
            ],
        };
        assert_eq!(round_trip(&spec), spec);
    }

    #[test]
    fn round_trip_all_pool_kernels() {
        for kernel in [
            PoolKernel::Mean,
            PoolKernel::Max,
            PoolKernel::Min,
            PoolKernel::Sum,
        ] {
            let spec = ObsSpec {
                entries: vec![ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: Some(PoolConfig {
                        kernel,
                        kernel_size: 2,
                        stride: 1,
                    }),
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                }],
            };
            assert_eq!(round_trip(&spec), spec, "failed for kernel {kernel:?}");
        }
    }

    #[test]
    fn version_rejection() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let mut bytes = serialize(&spec).unwrap();
        // Set version to 99
        bytes[4] = 99;
        bytes[5] = 0;
        let err = deserialize(&bytes).unwrap_err();
        assert!(format!("{err:?}").contains("unsupported version"));
    }

    #[test]
    fn truncated_bytes_error() {
        assert!(deserialize(&[]).is_err());
        assert!(deserialize(&[0, 0]).is_err());
        assert!(deserialize(b"MOBS").is_err());
    }

    #[test]
    fn serialize_rejects_overflow_entry_count() {
        // 65536 entries exceeds u16::MAX (65535), serialize must return Err.
        let entries: Vec<ObsEntry> = (0..=u16::MAX as u32)
            .map(|i| ObsEntry {
                field_id: FieldId(i),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            })
            .collect();
        let spec = ObsSpec { entries };
        // After fix, serialize returns Result and this must be Err.
        // Currently this silently truncates — the bug we're fixing.
        assert!(serialize(&spec).is_err());
    }

    #[test]
    fn deserialize_rejects_trailing_bytes() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let mut bytes = serialize(&spec).unwrap();
        // Append garbage trailing byte.
        bytes.push(0xFF);
        let err = deserialize(&bytes).unwrap_err();
        assert!(format!("{err:?}").contains("trailing"));
    }

    #[test]
    fn invalid_magic_error() {
        let bytes = b"BAD!\x01\x00\x00\x00";
        let err = deserialize(bytes).unwrap_err();
        assert!(format!("{err:?}").contains("invalid magic"));
    }

    #[test]
    fn serialize_rejects_negative_disk_radius_via_overflow() {
        // A radius of u32::MAX (> i32::MAX) should fail serialization.
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Disk {
                    center: smallvec![3, 4],
                    radius: u32::MAX,
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert!(serialize(&spec).is_err());
    }

    #[test]
    fn deserialize_rejects_negative_region_params() {
        // Manually craft bytes with a negative i32 for Disk radius.
        // This should be rejected on deserialization.
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Disk {
                    center: smallvec![3, 4],
                    radius: 5,
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let mut bytes = serialize(&spec).unwrap();
        // The Disk region params are: [center0, center1, radius] as i32 LE.
        // Find the radius param and overwrite with -1.
        // Header: 4 magic + 2 version + 2 n_entries = 8
        // Entry: 4 field_id + 1 region_tag + 2 n_params = 7
        // Params: 4*2 (center) = 8, then 4 (radius) at offset 8+7+8 = 23
        let radius_offset = 8 + 4 + 1 + 2 + 4 + 4; // 23
        bytes[radius_offset..radius_offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
        let err = deserialize(&bytes).unwrap_err();
        assert!(
            format!("{err:?}").contains("negative"),
            "expected 'negative' in error, got: {err:?}"
        );
    }

    #[test]
    fn serialize_rejects_overflow_agent_disk_radius() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk {
                    radius: i32::MAX as u32 + 1,
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert!(serialize(&spec).is_err());
    }

    #[test]
    fn serialize_rejects_overflow_agent_rect_half_extent() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec![u32::MAX],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        assert!(serialize(&spec).is_err());
    }
}
