/// Read a 24-bit stereo interleaved PCM file and downmix to mono f64.
///
/// Input format: raw 24-bit little-endian signed integers, interleaved L/R.
/// Each frame is 6 bytes: [L_b0, L_b1, L_b2, R_b0, R_b1, R_b2].
/// Output: mono samples normalized to [-1.0, 1.0].
pub fn read_pcm_stereo_to_mono(path: &std::path::Path) -> std::io::Result<Vec<f64>> {
    let data = std::fs::read(path)?;
    let frame_size = 6; // 3 bytes per sample * 2 channels
    if data.len() % frame_size != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "PCM file size {} is not a multiple of frame size {}",
                data.len(),
                frame_size
            ),
        ));
    }
    let num_frames = data.len() / frame_size;
    let norm = 8388608.0; // 2^23
    let mut mono = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let offset = i * frame_size;
        let left = decode_i24_le(&data[offset..offset + 3]) as f64 / norm;
        let right = decode_i24_le(&data[offset + 3..offset + 6]) as f64 / norm;
        mono.push((left + right) / 2.0);
    }
    Ok(mono)
}

fn decode_i24_le(bytes: &[u8]) -> i32 {
    let b0 = bytes[0] as u32;
    let b1 = bytes[1] as u32;
    let b2 = bytes[2] as u32;
    let val = b0 | (b1 << 8) | (b2 << 16);
    // Sign-extend from 24 to 32 bits
    if val & 0x800000 != 0 {
        (val | 0xFF000000) as i32
    } else {
        val as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_decode_i24_le_positive() {
        let val = decode_i24_le(&[0xE8, 0x03, 0x00]);
        assert_eq!(val, 1000);
    }

    #[test]
    fn test_decode_i24_le_negative() {
        let val = decode_i24_le(&[0xFF, 0xFF, 0xFF]);
        assert_eq!(val, -1);
    }

    #[test]
    fn test_decode_i24_le_zero() {
        let val = decode_i24_le(&[0x00, 0x00, 0x00]);
        assert_eq!(val, 0);
    }

    #[test]
    fn test_read_pcm_stereo_to_mono() {
        let dir = std::env::temp_dir().join("cwt_test_pcm");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.pcm");

        let mut file = std::fs::File::create(&path).unwrap();
        let write_i24 = |f: &mut std::fs::File, val: i32| {
            let bytes = val.to_le_bytes();
            f.write_all(&bytes[..3]).unwrap();
        };
        write_i24(&mut file, 1000);
        write_i24(&mut file, 3000);
        write_i24(&mut file, -2000);
        write_i24(&mut file, 2000);
        drop(file);

        let mono = read_pcm_stereo_to_mono(&path).unwrap();
        assert_eq!(mono.len(), 2);

        let norm = 8388608.0_f64;
        let expected_0 = (1000.0 + 3000.0) / 2.0 / norm;
        let expected_1 = (-2000.0 + 2000.0) / 2.0 / norm;
        assert!((mono[0] - expected_0).abs() < 1e-10);
        assert!((mono[1] - expected_1).abs() < 1e-10);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
