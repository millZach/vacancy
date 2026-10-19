//! The NTFS boot sector: the volume's geometry and where its file index starts.

use crate::le::{array, i64_at, u8_at, u16_at, u64_at};

/// What the boot sector says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boot {
    pub bytes_per_sector: u32,
    pub cluster_size: u32,
    pub total_clusters: u64,
    /// The first cluster of `$MFT`.
    pub mft_lcn: u64,
    pub record_size: u32,
    /// The volume serial number (all 64 bits, as `FILE_ID_INFO` gives it).
    pub serial: u64,
}

/// Why the boot sector isn't usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootError {
    /// Not an NTFS boot sector: another file system, or a volume BitLocker has locked.
    NotNtfs,
    /// NTFS's signature with geometry no NTFS volume has.
    Malformed,
}

/// Reads a boot sector (the volume's first 512 bytes or more).
pub fn parse(b: &[u8]) -> Result<Boot, BootError> {
    if array::<8>(b, 3) != Some(*b"NTFS    ") || array::<2>(b, 0x1FE) != Some([0x55, 0xAA]) {
        return Err(BootError::NotNtfs);
    }
    let m = BootError::Malformed;
    let bytes_per_sector = u32::from(u16_at(b, 0x0B).ok_or(m)?);
    if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
        return Err(m);
    }
    let spc = u8_at(b, 0x0D).ok_or(m)?;
    // Above 128, the value is a negative power of two (clusters of 64 KiB and more).
    let sectors_per_cluster: u32 = if spc <= 0x80 {
        u32::from(spc)
    } else {
        1u32.checked_shl(256 - u32::from(spc)).ok_or(m)?
    };
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return Err(m);
    }
    let cluster_size = bytes_per_sector.checked_mul(sectors_per_cluster).ok_or(m)?;
    if cluster_size > 2 << 20 {
        return Err(m);
    }
    let total_sectors = u64_at(b, 0x28).ok_or(m)?;
    let total_clusters = total_sectors / u64::from(sectors_per_cluster);
    let mft_lcn = u64_at(b, 0x30).ok_or(m)?;
    let per_record = i64::from(u8_at(b, 0x40).ok_or(m)? as i8);
    let record_size: u32 = if per_record > 0 {
        u32::try_from(per_record)
            .ok()
            .and_then(|n| n.checked_mul(cluster_size))
            .ok_or(m)?
    } else {
        1u32.checked_shl(u32::try_from(-per_record).map_err(|_| m)?)
            .ok_or(m)?
    };
    if !(512..=65536).contains(&record_size) || !record_size.is_power_of_two() {
        return Err(m);
    }
    if total_clusters == 0 || mft_lcn >= total_clusters {
        return Err(m);
    }
    let serial = i64_at(b, 0x48).ok_or(m)? as u64;
    Ok(Boot {
        bytes_per_sector,
        cluster_size,
        total_clusters,
        mft_lcn,
        record_size,
        serial,
    })
}

#[cfg(any(test, feature = "testkit"))]
pub(crate) fn sample(cluster_size: u32, total_clusters: u64, mft_lcn: u64) -> Vec<u8> {
    let mut b = vec![0u8; 512];
    b[3..11].copy_from_slice(b"NTFS    ");
    b[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
    b[0x0D] = (cluster_size / 512) as u8;
    b[0x28..0x30].copy_from_slice(&(total_clusters * u64::from(cluster_size / 512)).to_le_bytes());
    b[0x30..0x38].copy_from_slice(&mft_lcn.to_le_bytes());
    b[0x40] = (-10i8) as u8; // 1024-byte records
    b[0x48..0x50].copy_from_slice(&0x1234_5678_9ABC_DEF0u64.to_le_bytes());
    b[0x1FE] = 0x55;
    b[0x1FF] = 0xAA;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_geometry() {
        let b = sample(4096, 1000, 4);
        let boot = parse(&b).unwrap();
        assert_eq!(boot.cluster_size, 4096);
        assert_eq!(boot.total_clusters, 1000);
        assert_eq!(boot.record_size, 1024);
        assert_eq!(boot.mft_lcn, 4);
        assert_eq!(boot.serial, 0x1234_5678_9ABC_DEF0);
    }

    #[test]
    fn refuses_other_file_systems_and_bad_geometry() {
        let mut fve = sample(4096, 1000, 4);
        fve[3..11].copy_from_slice(b"-FVE-FS-");
        assert_eq!(
            parse(&fve),
            Err(BootError::NotNtfs),
            "a locked BitLocker volume"
        );
        assert_eq!(parse(&[0u8; 512]), Err(BootError::NotNtfs));
        assert_eq!(parse(&[0u8; 10]), Err(BootError::NotNtfs));
        let mut b = sample(4096, 1000, 4);
        b[0x0D] = 3;
        assert_eq!(parse(&b), Err(BootError::Malformed));
        let mut b = sample(4096, 1000, 4);
        b[0x30..0x38].copy_from_slice(&5000u64.to_le_bytes());
        assert_eq!(parse(&b), Err(BootError::Malformed), "the MFT past the end");
        let mut b = sample(4096, 1000, 4);
        b[0x40] = 100;
        assert_eq!(parse(&b), Err(BootError::Malformed), "records of 400 KiB");
    }
}
