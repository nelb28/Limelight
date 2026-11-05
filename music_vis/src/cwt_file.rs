use std::io::{self, Read};
use std::path::Path;

const MAGIC: &[u8; 4] = b"CWT1";

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
    global_max: f32,
}

impl CwtFile {
    pub fn new(
        num_scales: u32,
        num_frames: u32,
        sample_rate: u32,
        frame_rate: u32,
        frequencies: Vec<f32>,
        data: Vec<f32>,
    ) -> Self {
        let global_max = data.iter().cloned().fold(0.0f32, f32::max);
        Self {
            header: CwtHeader {
                num_scales,
                num_frames,
                sample_rate,
                frame_rate,
                frequencies,
            },
            data,
            global_max,
        }
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let mut f = std::fs::File::open(path)?;

        let mut magic = [0u8; 4];
        f.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad magic: expected CWT1, got {:?}", magic),
            ));
        }

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

        Ok(Self::new(num_scales, num_frames, sample_rate, frame_rate, frequencies, data))
    }

    /// Global maximum magnitude across all frames and scales.
    pub fn global_max(&self) -> f32 {
        self.global_max
    }

    /// Get magnitudes for a fractional frame position, linearly interpolated
    /// between the surrounding frames.
    /// `t` is the fractional frame index (e.g., 2.5 = halfway between frame 2 and 3).
    pub fn interpolated_frame(&self, t: f64) -> Vec<f32> {
        let n = self.header.num_scales as usize;
        let max_frame = self.header.num_frames as usize - 1;
        let frame_a = (t.floor() as usize).min(max_frame);
        let frame_b = (frame_a + 1).min(max_frame);
        let frac = (t - t.floor()) as f32;

        let mut result = Vec::with_capacity(n);
        for s in 0..n {
            let a = self.data[frame_a * n + s];
            let b = self.data[frame_b * n + s];
            result.push(a + (b - a) * frac);
        }
        result
    }
}
