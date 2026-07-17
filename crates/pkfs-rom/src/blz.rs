//! BLZ ("bottom LZ") decompression, as used for the NDS arm9 binary. The
//! compressed stream is processed back-to-front; the trailing 8 bytes encode
//! the compressed length, header length and size increase (see the compressor
//! in the pret decomp `tools/compstatic`).

/// Decompress a BLZ-compressed buffer (e.g. arm9). Returns the input unchanged
/// if it is not compressed (trailing `inc_len` of 0).
pub fn blz_decode(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    if n < 8 {
        return data.to_vec();
    }
    let inc_len = u32::from_le_bytes([data[n - 4], data[n - 3], data[n - 2], data[n - 1]]) as usize;
    if inc_len == 0 {
        return data.to_vec();
    }
    let hdr_len = data[n - 5] as usize;
    let enc_len =
        (data[n - 8] as usize) | ((data[n - 7] as usize) << 8) | ((data[n - 6] as usize) << 16);
    if enc_len < hdr_len || enc_len > n {
        return data.to_vec();
    }

    let dec_start = n - enc_len; // length of the uncompressed prefix
    let comp_len = enc_len - hdr_len;
    let out_len = n + inc_len;

    let mut out = vec![0u8; out_len];
    out[..n].copy_from_slice(data);

    let mut pak = dec_start + comp_len; // read cursor (moves down)
    let mut raw = out_len; // write cursor (moves down)
    let pak_end = dec_start;

    while pak > pak_end {
        pak -= 1;
        let mut flags = out[pak];
        for _ in 0..8 {
            if flags & 0x80 != 0 {
                if pak < 2 || raw < 1 {
                    return out;
                }
                pak -= 2;
                let hi = out[pak + 1] as usize;
                let lo = out[pak] as usize;
                let length = ((hi >> 4) & 0xF) + 3;
                let disp = (((hi & 0xF) << 8) | lo) + 3;
                for _ in 0..length {
                    if raw < 1 || raw + disp > out_len {
                        return out;
                    }
                    raw -= 1;
                    out[raw] = out[raw + disp];
                }
            } else {
                if pak < 1 || raw < 1 {
                    return out;
                }
                pak -= 1;
                raw -= 1;
                out[raw] = out[pak];
            }
            flags <<= 1;
            if pak <= pak_end {
                break;
            }
        }
    }
    out
}
