//! Exact integer BT.601 conversion, eight pixels at a time. Runtime dispatched.
use std::arch::x86_64::*;

#[target_feature(enable = "avx2")]
pub unsafe fn row(y: &[u8], uv: &[u8], rgba: &mut [u8]) -> usize {
    let pixels = y.len() / 8 * 8;
    assert!(uv.len() >= pixels && rgba.len() >= pixels * 4);
    // The caller validated plane lengths and the loop touches only complete
    // eight-pixel groups. Unaligned loads/stores do not require aligned Vecs.
    unsafe {
        let zero = _mm256_setzero_si256();
        let max = _mm256_set1_epi32(255);
        for x in (0..pixels).step_by(8) {
            let c = _mm256_max_epi32(
                zero,
                _mm256_sub_epi32(
                    _mm256_cvtepu8_epi32(_mm_loadl_epi64(y.as_ptr().add(x).cast())),
                    _mm256_set1_epi32(16),
                ),
            );
            let d = _mm256_setr_epi32(
                uv[x] as i32 - 128,
                uv[x] as i32 - 128,
                uv[x + 2] as i32 - 128,
                uv[x + 2] as i32 - 128,
                uv[x + 4] as i32 - 128,
                uv[x + 4] as i32 - 128,
                uv[x + 6] as i32 - 128,
                uv[x + 6] as i32 - 128,
            );
            let e = _mm256_setr_epi32(
                uv[x + 1] as i32 - 128,
                uv[x + 1] as i32 - 128,
                uv[x + 3] as i32 - 128,
                uv[x + 3] as i32 - 128,
                uv[x + 5] as i32 - 128,
                uv[x + 5] as i32 - 128,
                uv[x + 7] as i32 - 128,
                uv[x + 7] as i32 - 128,
            );
            let luma = _mm256_add_epi32(
                _mm256_mullo_epi32(c, _mm256_set1_epi32(298)),
                _mm256_set1_epi32(128),
            );
            let r = _mm256_min_epi32(
                max,
                _mm256_max_epi32(
                    zero,
                    _mm256_srai_epi32::<8>(_mm256_add_epi32(
                        luma,
                        _mm256_mullo_epi32(e, _mm256_set1_epi32(409)),
                    )),
                ),
            );
            let g = _mm256_min_epi32(
                max,
                _mm256_max_epi32(
                    zero,
                    _mm256_srai_epi32::<8>(_mm256_sub_epi32(
                        _mm256_sub_epi32(luma, _mm256_mullo_epi32(d, _mm256_set1_epi32(100))),
                        _mm256_mullo_epi32(e, _mm256_set1_epi32(208)),
                    )),
                ),
            );
            let b = _mm256_min_epi32(
                max,
                _mm256_max_epi32(
                    zero,
                    _mm256_srai_epi32::<8>(_mm256_add_epi32(
                        luma,
                        _mm256_mullo_epi32(d, _mm256_set1_epi32(516)),
                    )),
                ),
            );
            let packed = _mm256_or_si256(
                _mm256_or_si256(r, _mm256_slli_epi32::<8>(g)),
                _mm256_or_si256(
                    _mm256_slli_epi32::<16>(b),
                    _mm256_set1_epi32(0xff000000u32 as i32),
                ),
            );
            _mm256_storeu_si256(rgba.as_mut_ptr().add(x * 4).cast(), packed);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    #[test]
    fn vector_conversion_exactly_matches_scalar_for_all_byte_values() {
        if !std::is_x86_feature_detected!("avx2") {
            return;
        }
        for seed in 0..256usize {
            let y: Vec<_> = (0..256).map(|i| ((i + seed) % 256) as u8).collect();
            let uv: Vec<_> = (0..256)
                .map(|i| ((i * 73 + seed * 17) % 256) as u8)
                .collect();
            let mut out = vec![0u8; 1024];
            unsafe {
                super::row(&y, &uv, &mut out);
            }
            for x in 0..256 {
                let c = (y[x] as i32 - 16).max(0);
                let d = uv[x / 2 * 2] as i32 - 128;
                let e = uv[x / 2 * 2 + 1] as i32 - 128;
                assert_eq!(
                    &out[x * 4..x * 4 + 4],
                    &[
                        ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8,
                        ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8,
                        ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8,
                        255
                    ]
                );
            }
        }
    }
}
