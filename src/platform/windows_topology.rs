//! Bounds-checked parsing of the 64-bit GetLogicalProcessorInformationEx buffer.
//! Record offsets follow the Windows SDK, independently exercised by the native
//! PowerShell oracle. Multiple processor groups remain unsupported by placement.

use std::collections::BTreeMap;

type Entry = (usize, String, Option<u32>);

fn bytes<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], String> {
    data.get(offset..offset + N)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| "truncated Windows topology record".into())
}

fn word(data: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes(data, offset)?))
}

fn dword(data: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes(data, offset)?))
}

fn mask(data: &[u8], offset: usize) -> Result<u64, String> {
    // All callers supply constant offsets inside bounded records.
    let value = u64::from_le_bytes(bytes(data, offset)?);
    if word(data, offset + 8)? != 0 || value == 0 {
        return Err("Windows topology requires one nonempty processor group".into());
    }
    Ok(value)
}

pub(super) fn decode(mut data: &[u8]) -> Result<Vec<Entry>, String> {
    let mut cores = BTreeMap::new();
    let mut caches = Vec::new();
    let mut active = None;
    while !data.is_empty() {
        let relation = dword(data, 0)?;
        let size = dword(data, 4)? as usize;
        if size < 8 || size > data.len() {
            return Err("invalid Windows topology record size".into());
        }
        let record = &data[..size];
        data = &data[size..];
        match relation {
            0 => {
                if word(record, 30)? != 1 {
                    return Err("Windows core must have exactly one group".into());
                }
                let cpus = mask(record, 32)?;
                let rank = bytes::<1>(record, 9)?[0];
                for cpu in 0..64 {
                    if cpus & (1u64 << cpu) != 0 && cores.insert(cpu, rank).is_some() {
                        return Err("duplicate Windows logical processor".into());
                    }
                }
            }
            2 => {
                // Zero is the legacy SDK's single GroupMask spelling.
                if word(record, 38)? > 1 {
                    return Err("Windows multi-group cache is unsupported".into());
                }
                let cpus = mask(record, 40)?;
                let level = bytes::<1>(record, 8)?[0];
                let size = dword(record, 12)?;
                let kind = dword(record, 16)?;
                if level == 0 || size == 0 || kind > 3 {
                    return Err("invalid Windows cache descriptor".into());
                }
                if kind == 0 || kind == 2 {
                    caches.push((cpus, level, size));
                }
            }
            4 => {
                if active.is_some() || word(record, 10)? != 1 {
                    return Err("Windows multiple processor groups are unsupported".into());
                }
                let cpus = u64::from_le_bytes(bytes(record, 72)?);
                if cpus == 0 || cpus.count_ones() != u32::from(bytes::<1>(record, 33)?[0]) {
                    return Err("inconsistent Windows active processor mask".into());
                }
                active = Some(cpus);
            }
            _ => {} // NUMA/package/die/module records do not define our classes.
        }
    }
    let active = active.ok_or("Windows processor-group record is missing")?;
    let seen = cores.keys().fold(0u64, |mask, cpu| mask | (1u64 << cpu));
    if seen != active || caches.iter().any(|(cpus, _, _)| cpus & !active != 0) {
        return Err("Windows core/cache inventory disagrees with the active group".into());
    }
    cores
        .into_iter()
        .map(|(cpu, rank)| {
            let mut profile: Vec<_> = caches
                .iter()
                .filter(|(cpus, _, _)| cpus & (1u64 << cpu) != 0)
                .map(|(_, level, size)| (*level, *size))
                .collect();
            let last = profile
                .iter()
                .map(|(level, _)| *level)
                .max()
                .ok_or("Windows processor has no data/unified cache information")?;
            profile.retain(|(level, _)| *level == last);
            profile.sort_unstable();
            profile.dedup();
            let mut name = format!("windows_efficiency{rank}");
            for (level, size) in profile {
                name.push_str(&format!("_l{level}bytes{size}"));
            }
            Ok((cpu, name, Some(u32::from(rank))))
        })
        .collect()
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub(super) fn discover() -> Result<Vec<Entry>, String> {
    decode(&native::read().map_err(|e| format!("Windows topology query failed: {e}"))?)
}

// The allocation/retry protocol is safe and injectable for failure controls.
fn read_with(
    mut query: impl FnMut(Option<&mut [u64]>, &mut u32) -> std::io::Result<()>,
) -> std::io::Result<Vec<u8>> {
    use std::io;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    let mut length = 0u32;
    match query(None, &mut length) {
        Err(error) if error.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER) => {}
        Err(error) => return Err(error),
        Ok(()) => return Err(io::Error::other("unexpected topology sizing success")),
    }
    // Hotplug can change the size between calls. Bound allocation and retries.
    for _ in 0..3 {
        if !(8..=16 * 1024 * 1024).contains(&length) {
            return Err(io::Error::other("invalid topology buffer size"));
        }
        let capacity = length as usize;
        let words = capacity.div_ceil(8);
        let mut storage = Vec::<u64>::new();
        storage.try_reserve_exact(words).map_err(io::Error::other)?;
        storage.resize(words, 0);
        match query(Some(&mut storage), &mut length) {
            Ok(()) => {
                if length < 8 || length as usize > capacity {
                    return Err(io::Error::other("invalid topology readback length"));
                }
                return Ok(storage
                    .into_iter()
                    .flat_map(u64::to_ne_bytes)
                    .take(length as usize)
                    .collect());
            }
            Err(error) if error.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER) => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "topology changed during every query attempt",
    ))
}

// User-approved Windows topology primitive. Checked slices parse every byte;
// no native structs, uninitialized memory or borrowed union views escape.
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
#[allow(unsafe_code)]
mod native {
    use std::ffi::c_void;
    use std::io;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLogicalProcessorInformationEx(
            relationship: i32,
            buffer: *mut c_void,
            length: *mut u32,
        ) -> i32;
    }

    pub(super) fn read() -> io::Result<Vec<u8>> {
        super::read_with(|storage, length| {
            let pointer = match storage {
                Some(storage) => {
                    // Defend the native boundary independently of the caller.
                    if *length as usize > std::mem::size_of_val(storage) {
                        return Err(io::Error::other("topology capacity exceeds allocation"));
                    }
                    storage.as_mut_ptr().cast()
                }
                None => {
                    if *length != 0 {
                        return Err(io::Error::other("nonzero null topology buffer"));
                    }
                    std::ptr::null_mut()
                }
            };
            // SAFETY: the null/zero pair is the documented size query. Otherwise
            // u64 storage supplies the ARM64 SDK's 8-byte alignment and at least
            // `length` initialized writable bytes (checked above). The exclusive
            // storage and length borrows remain live for the synchronous call;
            // the OS retains neither pointer. RELATION_ALL is the SDK enum value.
            if unsafe { GetLogicalProcessorInformationEx(0xffff, pointer, length) } != 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_buffer() -> Vec<u8> {
        include_str!("../../tests/fixtures/windows_arm64_topology.hex")
            .split_whitespace()
            .flat_map(|line| {
                (0..line.len())
                    .step_by(2)
                    .map(move |i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
            })
            .collect()
    }

    #[test]
    fn query_protocol_bounds_retries_and_validates_readback() {
        use std::io;
        let insufficient = || io::Error::from_raw_os_error(122);
        let mut calls = 0;
        let result = read_with(|buffer, length| {
            calls += 1;
            if buffer.is_none() {
                *length = 8;
                return Err(insufficient());
            }
            if calls == 2 {
                *length = 16;
                return Err(insufficient());
            }
            let buffer = buffer.unwrap();
            assert_eq!(buffer.len(), 2);
            buffer[0] = u64::from_ne_bytes([1; 8]);
            buffer[1] = u64::from_ne_bytes([2; 8]);
            *length = 12;
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(result, [vec![1; 8], vec![2; 4]].concat());
        let mut calls = 0;
        assert!(
            read_with(|_, length| {
                calls += 1;
                *length = 8;
                Err(insufficient())
            })
            .is_err()
        );
        assert_eq!(calls, 4);
        assert!(read_with(|_, _| Ok(())).is_err());
        assert_eq!(
            read_with(|_, _| Err(io::Error::from_raw_os_error(5)))
                .unwrap_err()
                .raw_os_error(),
            Some(5)
        );
        for bad in [0, 7, 16 * 1024 * 1024 + 1, u32::MAX] {
            let mut calls = 0;
            assert!(
                read_with(|_, length| {
                    calls += 1;
                    *length = bad;
                    Err(insufficient())
                })
                .is_err()
            );
            assert_eq!(calls, 1);
        }
        for bad in [0, 7, 9] {
            assert!(
                read_with(|buffer, length| {
                    if buffer.is_none() {
                        *length = 8;
                        Err(insufficient())
                    } else {
                        *length = bad;
                        Ok(())
                    }
                })
                .is_err()
            );
        }
    }

    #[test]
    fn native_snapdragon_matches_independent_windows_oracle() {
        let entries = decode(&native_buffer()).unwrap();
        assert_eq!(entries.len(), 12);
        for (cpu, name, rank) in entries {
            let expected = if cpu < 4 { 0 } else { 1 };
            assert_eq!(rank, Some(expected));
            assert_eq!(
                name,
                format!("windows_efficiency{expected}_l2bytes12582912")
            );
        }
    }

    #[test]
    fn equal_performance_rank_preserves_unequal_last_level_caches() {
        let mut data = native_buffer();
        let mut offset = 0;
        let mut changed = false;
        while offset < data.len() {
            let relation = dword(&data, offset).unwrap();
            let size = dword(&data, offset + 4).unwrap() as usize;
            if relation == 0 {
                data[offset + 9] = 0;
            }
            if relation == 2 && data[offset + 8] == 2 && !changed {
                data[offset + 12..offset + 16].copy_from_slice(&33554432u32.to_le_bytes());
                changed = true;
            }
            offset += size;
        }
        assert!(changed);
        for (cpu, name, rank) in decode(&data).unwrap() {
            assert_eq!(rank, Some(0));
            let size = if cpu < 4 { 33554432 } else { 12582912 };
            assert_eq!(name, format!("windows_efficiency0_l2bytes{size}"));
        }
        let mut groups = native_buffer();
        groups[2338..2340].copy_from_slice(&2u16.to_le_bytes());
        assert!(decode(&groups).is_err());
    }

    #[test]
    fn truncation_and_inconsistent_records_fail_closed() {
        let data = native_buffer();
        // Every cut through a native record must refuse; complete prefix records
        // also refuse when the mandatory group/core/cache inventory is incomplete.
        for end in 0..data.len() {
            assert!(decode(&data[..end]).is_err(), "accepted prefix {end}");
        }
        let mut zero_record = data.clone();
        zero_record[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&zero_record).is_err());
        let mut oversized = data.clone();
        oversized[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&oversized).is_err());
        let mut duplicate = data.clone();
        // The captured buffer starts with a 48-byte package record.
        let core = &data[48..96];
        assert_eq!(dword(core, 0).unwrap(), 0);
        duplicate.extend_from_slice(core);
        assert!(decode(&duplicate).is_err());
    }
}
