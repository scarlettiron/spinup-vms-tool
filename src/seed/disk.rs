//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Building the seed disk in-process: a small disk with one FAT32 partition whose
//volume label is the whole contract. CIDATA is found by cloud-init, OEMDRV by
//Anaconda. Writing the bytes ourselves needs no admin rights or disk tools, and
//works the same on every computer.
//
//Hyper-V wants a .vhd: a "fixed" VHD is just the raw disk with a 512-byte
//footer on the end, so no conversion tool is needed either.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

pub const SEED_SIZE: u64 = 256 << 20;
const SECTOR: u64 = 512;
//The partition starts 1 MiB in, as every modern partitioning tool does.
const PARTITION_START: u64 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    Raw,
    Vhd,
}

//Writes a seed disk holding `files` (path inside the volume, contents).
pub fn write(
    path: &Path,
    label: &str,
    files: &[(String, Vec<u8>)],
    format: DiskFormat,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.set_len(SEED_SIZE)?;
    write_mbr(&mut file, SEED_SIZE)?;

    let partition_bytes = SEED_SIZE - PARTITION_START * SECTOR;
    {
        let slice = Slice::new(&mut file, PARTITION_START * SECTOR, partition_bytes);
        let options = fatfs::FormatVolumeOptions::new()
            .fat_type(fatfs::FatType::Fat32)
            .volume_id(now_since_2000() ^ 0x5eed_0000)
            .volume_label(volume_label(label)?);
        fatfs::format_volume(slice, options).context("could not format the seed disk")?;
    }
    {
        let slice = Slice::new(&mut file, PARTITION_START * SECTOR, partition_bytes);
        let fs = fatfs::FileSystem::new(slice, fatfs::FsOptions::new())
            .context("could not open the seed disk's file system")?;
        let root = fs.root_dir();
        for (name, contents) in files {
            let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
            let Some((leaf, folders)) = parts.split_last() else {
                continue;
            };
            let mut dir = root.clone();
            for folder in folders {
                dir = match dir.open_dir(folder) {
                    Ok(existing) => existing,
                    Err(_) => dir.create_dir(folder)?,
                };
            }
            let mut out = dir.create_file(leaf)?;
            out.truncate()?;
            out.write_all(contents)?;
            out.flush()?;
        }
        drop(root);
        fs.unmount()?;
    }
    if format == DiskFormat::Vhd {
        file.seek(SeekFrom::End(0))?;
        file.write_all(&vhd_footer(SEED_SIZE, now_since_2000(), &unique_id(path)))?;
    }
    file.sync_all()?;
    Ok(())
}

//FAT labels are 11 bytes of upper-case ASCII, padded with spaces.
fn volume_label(label: &str) -> Result<[u8; 11]> {
    let upper = label.to_uppercase();
    if upper.is_empty()
        || upper.len() > 11
        || !upper.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        bail!("the seed label '{label}' must be 1 to 11 letters, digits or underscores");
    }
    let mut bytes = [b' '; 11];
    bytes[..upper.len()].copy_from_slice(upper.as_bytes());
    Ok(bytes)
}

//An MBR partition table with one FAT32 (LBA) partition filling the disk.
fn write_mbr(file: &mut File, size: u64) -> Result<()> {
    let mut sector = [0u8; 512];
    let signature = now_since_2000() ^ 0x564d_594e;
    sector[440..444].copy_from_slice(&signature.to_le_bytes());
    let entry = &mut sector[446..462];
    entry[0] = 0x00;
    entry[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[4] = 0x0c;
    entry[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[8..12].copy_from_slice(&(PARTITION_START as u32).to_le_bytes());
    let sectors = (size / SECTOR - PARTITION_START) as u32;
    entry[12..16].copy_from_slice(&sectors.to_le_bytes());
    sector[510] = 0x55;
    sector[511] = 0xaa;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&sector)?;
    Ok(())
}

//The 512-byte footer that makes a raw disk a fixed VHD (Microsoft's VHD spec).
pub fn vhd_footer(size: u64, timestamp: u32, id: &[u8; 16]) -> [u8; 512] {
    let mut f = [0u8; 512];
    f[0..8].copy_from_slice(b"conectix");
    f[8..12].copy_from_slice(&2u32.to_be_bytes());
    f[12..16].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    f[16..24].copy_from_slice(&u64::MAX.to_be_bytes());
    f[24..28].copy_from_slice(&timestamp.to_be_bytes());
    f[28..32].copy_from_slice(b"vmyn");
    f[32..36].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    f[36..40].copy_from_slice(b"Wi2k");
    f[40..48].copy_from_slice(&size.to_be_bytes());
    f[48..56].copy_from_slice(&size.to_be_bytes());
    let (cylinders, heads, sectors) = chs(size);
    f[56..58].copy_from_slice(&cylinders.to_be_bytes());
    f[58] = heads;
    f[59] = sectors;
    f[60..64].copy_from_slice(&2u32.to_be_bytes());
    f[68..84].copy_from_slice(id);
    let sum: u32 = f.iter().map(|b| u32::from(*b)).sum();
    f[64..68].copy_from_slice(&(!sum).to_be_bytes());
    f
}

//The disk geometry the VHD spec asks for, from its own algorithm.
fn chs(size: u64) -> (u16, u8, u8) {
    let mut total = size / SECTOR;
    total = total.min(65535 * 16 * 255);
    let (sectors, heads, cylinder_heads);
    if total >= 65535 * 16 * 63 {
        sectors = 255u64;
        heads = 16u64;
        cylinder_heads = total / sectors;
    } else {
        let mut s = 17u64;
        let mut ch = total / s;
        let mut h = ch.div_ceil(1024);
        if h < 4 {
            h = 4;
        }
        if ch >= h * 1024 || h > 16 {
            s = 31;
            h = 16;
            ch = total / s;
        }
        if ch >= h * 1024 {
            s = 63;
            h = 16;
            ch = total / s;
        }
        sectors = s;
        heads = h;
        cylinder_heads = ch;
    }
    ((cylinder_heads / heads) as u16, heads as u8, sectors as u8)
}

fn now_since_2000() -> u32 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    since_epoch.saturating_sub(946_684_800) as u32
}

//A disk id that differs per file and per run.
fn unique_id(path: &Path) -> [u8; 16] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .to_le_bytes(),
    );
    let digest = hasher.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

//A window onto part of a file, so the FAT code sees only the partition.
struct Slice<'a> {
    file: &'a mut File,
    start: u64,
    len: u64,
    position: u64,
}

impl<'a> Slice<'a> {
    fn new(file: &'a mut File, start: u64, len: u64) -> Self {
        Slice {
            file,
            start,
            len,
            position: 0,
        }
    }
}

impl Read for Slice<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.position) as usize;
        let want = buf.len().min(left);
        if want == 0 {
            return Ok(0);
        }
        self.file
            .seek(SeekFrom::Start(self.start + self.position))?;
        let read = self.file.read(&mut buf[..want])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Write for Slice<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.position) as usize;
        let want = buf.len().min(left);
        if want == 0 && !buf.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "the seed partition is full",
            ));
        }
        self.file
            .seek(SeekFrom::Start(self.start + self.position))?;
        let written = self.file.write(&buf[..want])?;
        self.position += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for Slice<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(offset) => offset as i128,
            SeekFrom::End(offset) => self.len as i128 + offset as i128,
            SeekFrom::Current(offset) => self.position as i128 + offset as i128,
        };
        if next < 0 || next > self.len as i128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside the partition",
            ));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_disk_holds_the_files_under_its_label() {
        let path = std::env::temp_dir().join(format!("vmseed-seed-{}.img", std::process::id()));
        let files = vec![
            ("ks.cfg".to_string(), b"text\n".to_vec()),
            ("bootstrap/assets/x.sh".to_string(), b"echo hi\n".to_vec()),
        ];
        write(&path, "OEMDRV", &files, DiskFormat::Raw).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), SEED_SIZE);

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut mbr = [0u8; 512];
        file.read_exact(&mut mbr).unwrap();
        assert_eq!(&mbr[510..], &[0x55, 0xaa]);
        assert_eq!(mbr[450], 0x0c);

        let slice = Slice::new(
            &mut file,
            PARTITION_START * SECTOR,
            SEED_SIZE - PARTITION_START * SECTOR,
        );
        let fs = fatfs::FileSystem::new(slice, fatfs::FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), fatfs::FatType::Fat32);
        assert_eq!(fs.volume_label(), "OEMDRV");
        assert_eq!(
            fs.read_volume_label_from_root_dir().unwrap().as_deref(),
            Some("OEMDRV")
        );
        let mut text = String::new();
        fs.root_dir()
            .open_file("bootstrap/assets/x.sh")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "echo hi\n");
        drop(fs);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn vhd_footers_follow_the_spec() {
        let footer = vhd_footer(256 << 20, 0, &[7; 16]);
        assert_eq!(&footer[0..8], b"conectix");
        assert_eq!(
            u32::from_be_bytes(footer[60..64].try_into().unwrap()),
            2,
            "fixed disk"
        );
        //The checksum is the one's complement of the sum of every other byte.
        let mut copy = footer;
        copy[64..68].fill(0);
        let sum: u32 = copy.iter().map(|b| u32::from(*b)).sum();
        assert_eq!(u32::from_be_bytes(footer[64..68].try_into().unwrap()), !sum);
        //256 MiB works out at 16 heads and 63 sectors per track.
        let (c, h, s) = chs(256 << 20);
        assert!(u64::from(c) * u64::from(h) * u64::from(s) <= (256 << 20) / 512);
        assert_eq!((c, h, s), (520, 16, 63));
    }

    #[test]
    fn labels_are_checked() {
        assert!(volume_label("cidata").is_ok());
        assert!(volume_label("way-too-long-label").is_err());
    }
}
