use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

/// CWT1: header + magnitudes. CWT2: CWT1 + phase deviation (cents), same layout.
pub const MAGIC: &[u8; 4] = b"CWT2";
const MAGIC_V1: &[u8; 4] = b"CWT1";

pub struct CwtHeader {
    pub num_scales: u32,
    pub num_frames: u32,
    pub sample_rate: u32,
    pub frame_rate: u32,
    pub frequencies: Vec<f32>,
}

pub struct CwtFile {
    pub header: CwtHeader,
    /// Row-major: data[frame_idx * num_scales + scale_idx]
    pub data: Vec<f32>,
    /// Instantaneous-frequency deviation from each bin's center, in cents.
    /// Same layout as `data`. Empty for CWT1 files.
    pub phase_dev: Vec<f32>,
}

impl CwtFile {
    pub fn new(
        num_scales: u32,
        num_frames: u32,
        sample_rate: u32,
        frame_rate: u32,
        frequencies: Vec<f32>,
        data: Vec<f32>,
        phase_dev: Vec<f32>,
    ) -> Self {
        assert_eq!(frequencies.len(), num_scales as usize);
        assert_eq!(data.len(), (num_scales * num_frames) as usize);
        assert!(phase_dev.is_empty() || phase_dev.len() == data.len());
        Self {
            header: CwtHeader {
                num_scales,
                num_frames,
                sample_rate,
                frame_rate,
                frequencies,
            },
            data,
            phase_dev,
        }
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let mut f = BufWriter::new(std::fs::File::create(path)?);
        f.write_all(MAGIC)?;
        f.write_all(&self.header.num_scales.to_le_bytes())?;
        f.write_all(&self.header.num_frames.to_le_bytes())?;
        f.write_all(&self.header.sample_rate.to_le_bytes())?;
        f.write_all(&self.header.frame_rate.to_le_bytes())?;
        for &freq in &self.header.frequencies {
            f.write_all(&freq.to_le_bytes())?;
        }
        for &val in self.data.iter().chain(self.phase_dev.iter()) {
            f.write_all(&val.to_le_bytes())?;
        }
        f.flush()
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let mut f = std::fs::File::open(path)?;

        let mut magic = [0u8; 4];
        f.read_exact(&mut magic)?;
        let has_phase = match &magic {
            m if m == MAGIC => true,
            m if m == MAGIC_V1 => false,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("bad magic: expected CWT1/CWT2, got {:?}", magic),
                ))
            }
        };

        let read_u32 = |f: &mut std::fs::File| -> io::Result<u32> {
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(u32::from_le_bytes(buf))
        };
        let read_f32 = |f: &mut std::fs::File| -> io::Result<f32> {
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(f32::from_le_bytes(buf))
        };

        let num_scales = read_u32(&mut f)?;
        let num_frames = read_u32(&mut f)?;
        let sample_rate = read_u32(&mut f)?;
        let frame_rate = read_u32(&mut f)?;

        let mut frequencies = Vec::with_capacity(num_scales as usize);
        for _ in 0..num_scales {
            frequencies.push(read_f32(&mut f)?);
        }

        let data_len = (num_scales * num_frames) as usize;
        let mut data = Vec::with_capacity(data_len);
        for _ in 0..data_len {
            data.push(read_f32(&mut f)?);
        }
        let mut phase_dev = Vec::new();
        if has_phase {
            phase_dev.reserve(data_len);
            for _ in 0..data_len {
                phase_dev.push(read_f32(&mut f)?);
            }
        }

        Ok(Self::new(num_scales, num_frames, sample_rate, frame_rate, frequencies, data, phase_dev))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        let frequencies = vec![20.0, 100.0, 1000.0, 10000.0];
        let data = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let phase: Vec<f32> = data.iter().map(|v| v * -100.0).collect();
        let cwt = CwtFile::new(4, 2, 96000, 60, frequencies.clone(), data.clone(), phase.clone());

        let dir = std::env::temp_dir().join("cwt_test_file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.cwt");
        cwt.write(&path).unwrap();

        let loaded = CwtFile::read(&path).unwrap();
        assert_eq!(loaded.header.num_scales, 4);
        assert_eq!(loaded.header.num_frames, 2);
        assert_eq!(loaded.header.sample_rate, 96000);
        assert_eq!(loaded.header.frame_rate, 60);
        assert_eq!(loaded.header.frequencies, frequencies);
        assert_eq!(loaded.data.len(), data.len());
        for (a, b) in loaded.data.iter().zip(data.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
        assert_eq!(loaded.phase_dev, phase);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_bad_magic() {
        let dir = std::env::temp_dir().join("cwt_test_bad_magic");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.cwt");
        std::fs::write(&path, b"NOPE").unwrap();

        let result = CwtFile::read(&path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
