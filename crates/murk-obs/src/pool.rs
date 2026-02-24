//! Spatial pooling operations for observation extraction.
//!
//! Pooling reduces the spatial resolution of a gathered observation
//! buffer by sliding a window and computing an aggregate (mean, max,
//! min, or sum) over valid cells within each window.

use crate::spec::{PoolConfig, PoolKernel};
use murk_core::error::ObsError;

/// Apply 2D spatial pooling to a gathered observation buffer.
///
/// `input` is the flat gather buffer in row-major order.
/// `input_mask` has 1 for valid cells, 0 for padding.
/// `input_shape` is `[H, W]`.
///
/// Returns `(output, output_mask, output_shape)` where output_shape
/// is `[(H - kernel_size) / stride + 1, (W - kernel_size) / stride + 1]`.
pub fn pool_2d(
    input: &[f32],
    input_mask: &[u8],
    input_shape: &[usize],
    config: &PoolConfig,
) -> (Vec<f32>, Vec<u8>, Vec<usize>) {
    let (out_h, out_w) = pool_2d_output_shape(input_shape, config);
    let out_len = out_h * out_w;
    let mut output = vec![0.0f32; out_len];
    let mut output_mask = vec![0u8; out_len];
    pool_2d_into(
        input,
        input_mask,
        input_shape,
        config,
        &mut output,
        &mut output_mask,
    )
    .expect("pool_2d: invalid arguments");
    (output, output_mask, vec![out_h, out_w])
}

/// Return the output shape for a 2D pool operation.
pub fn pool_2d_output_shape(input_shape: &[usize], config: &PoolConfig) -> (usize, usize) {
    assert_eq!(input_shape.len(), 2, "pool_2d requires 2D input shape");
    let h = input_shape[0];
    let w = input_shape[1];

    let ks = config.kernel_size;
    let stride = config.stride;
    assert!(ks > 0, "kernel_size must be > 0");
    assert!(stride > 0, "stride must be > 0");

    let out_h = if h >= ks { (h - ks) / stride + 1 } else { 0 };
    let out_w = if w >= ks { (w - ks) / stride + 1 } else { 0 };
    (out_h, out_w)
}

/// Apply 2D pooling into caller-provided output buffers (no allocation).
///
/// Returns `(out_h, out_w)` on success. Returns `Err` if shapes or
/// buffer sizes are invalid (avoids panicking on FFI execution paths).
pub fn pool_2d_into(
    input: &[f32],
    input_mask: &[u8],
    input_shape: &[usize],
    config: &PoolConfig,
    output: &mut [f32],
    output_mask: &mut [u8],
) -> Result<(usize, usize), ObsError> {
    if input_shape.len() != 2 {
        return Err(ObsError::InvalidObsSpec {
            reason: format!("pool_2d_into requires 2D input shape, got {}", input_shape.len()),
        });
    }
    let h = input_shape[0];
    let w = input_shape[1];
    if input.len() != h * w {
        return Err(ObsError::ExecutionFailed {
            reason: format!("input length {} != {}×{}", input.len(), h, w),
        });
    }
    if input_mask.len() != h * w {
        return Err(ObsError::ExecutionFailed {
            reason: format!("input_mask length {} != {}×{}", input_mask.len(), h, w),
        });
    }

    let (out_h, out_w) = pool_2d_output_shape(input_shape, config);
    let ks = config.kernel_size;
    let stride = config.stride;
    let out_len = out_h * out_w;
    if output.len() < out_len {
        return Err(ObsError::ExecutionFailed {
            reason: format!("output buffer too small: {} < {}", output.len(), out_len),
        });
    }
    if output_mask.len() < out_len {
        return Err(ObsError::ExecutionFailed {
            reason: format!("output_mask buffer too small: {} < {}", output_mask.len(), out_len),
        });
    }

    for oh in 0..out_h {
        for ow in 0..out_w {
            let r0 = oh * stride;
            let c0 = ow * stride;
            let out_idx = oh * out_w + ow;

            let mut valid_count = 0u32;
            let mut accum = match config.kernel {
                PoolKernel::Max => f32::NEG_INFINITY,
                PoolKernel::Min => f32::INFINITY,
                PoolKernel::Mean | PoolKernel::Sum => 0.0,
            };

            for kr in 0..ks {
                for kc in 0..ks {
                    let r = r0 + kr;
                    let c = c0 + kc;
                    let idx = r * w + c;
                    if input_mask[idx] == 1 {
                        let val = input[idx];
                        // For Max/Min, skip NaN values to avoid emitting
                        // sentinel infinities as "valid" results.
                        if matches!(config.kernel, PoolKernel::Max | PoolKernel::Min)
                            && val.is_nan()
                        {
                            continue;
                        }
                        valid_count += 1;
                        match config.kernel {
                            PoolKernel::Mean | PoolKernel::Sum => accum += val,
                            PoolKernel::Max => {
                                if val > accum {
                                    accum = val;
                                }
                            }
                            PoolKernel::Min => {
                                if val < accum {
                                    accum = val;
                                }
                            }
                        }
                    }
                }
            }

            if valid_count > 0 {
                output_mask[out_idx] = 1;
                output[out_idx] = match config.kernel {
                    PoolKernel::Mean => accum / valid_count as f32,
                    PoolKernel::Max | PoolKernel::Min | PoolKernel::Sum => accum,
                };
            } else {
                output_mask[out_idx] = 0;
                output[out_idx] = 0.0;
            }
        }
    }

    Ok((out_h, out_w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_cfg(kernel: PoolKernel, kernel_size: usize, stride: usize) -> PoolConfig {
        PoolConfig {
            kernel,
            kernel_size,
            stride,
        }
    }

    #[test]
    fn mean_pool_2x2_stride2_on_4x4() {
        // 4x4 input, values 1..16
        let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let mask = vec![1u8; 16];
        let cfg = pool_cfg(PoolKernel::Mean, 2, 2);

        let (output, out_mask, out_shape) = pool_2d(&input, &mask, &[4, 4], &cfg);
        assert_eq!(out_shape, vec![2, 2]);
        assert_eq!(out_mask, vec![1, 1, 1, 1]);

        // Top-left: (1+2+5+6)/4 = 3.5
        assert!((output[0] - 3.5).abs() < 1e-6);
        // Top-right: (3+4+7+8)/4 = 5.5
        assert!((output[1] - 5.5).abs() < 1e-6);
        // Bottom-left: (9+10+13+14)/4 = 11.5
        assert!((output[2] - 11.5).abs() < 1e-6);
        // Bottom-right: (11+12+15+16)/4 = 13.5
        assert!((output[3] - 13.5).abs() < 1e-6);
    }

    #[test]
    fn max_pool_2x2_stride2() {
        let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let mask = vec![1u8; 16];
        let cfg = pool_cfg(PoolKernel::Max, 2, 2);

        let (output, _, out_shape) = pool_2d(&input, &mask, &[4, 4], &cfg);
        assert_eq!(out_shape, vec![2, 2]);
        assert_eq!(output, vec![6.0, 8.0, 14.0, 16.0]);
    }

    #[test]
    fn min_pool_2x2_stride2() {
        let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let mask = vec![1u8; 16];
        let cfg = pool_cfg(PoolKernel::Min, 2, 2);

        let (output, _, _) = pool_2d(&input, &mask, &[4, 4], &cfg);
        assert_eq!(output, vec![1.0, 3.0, 9.0, 11.0]);
    }

    #[test]
    fn sum_pool_2x2_stride2() {
        let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let mask = vec![1u8; 16];
        let cfg = pool_cfg(PoolKernel::Sum, 2, 2);

        let (output, _, _) = pool_2d(&input, &mask, &[4, 4], &cfg);
        // Top-left: 1+2+5+6=14
        assert_eq!(output, vec![14.0, 22.0, 46.0, 54.0]);
    }

    #[test]
    fn partial_valid_mask_mean() {
        // 4x4 input, but some cells masked out
        let input = vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ];
        let mask = vec![1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1];
        let cfg = pool_cfg(PoolKernel::Mean, 2, 2);

        let (output, out_mask, _) = pool_2d(&input, &mask, &[4, 4], &cfg);
        // Top-left: (1+5+6)/3 = 4.0 (cell [0,1]=2 masked out)
        assert!((output[0] - 4.0).abs() < 1e-6);
        assert_eq!(out_mask[0], 1);
        // Top-right: (3+4+8)/3 = 5.0 (cell [1,2]=7 masked out)
        assert!((output[1] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn all_masked_window_gives_zero() {
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let mask = vec![0, 0, 0, 0];
        let cfg = pool_cfg(PoolKernel::Mean, 2, 2);

        let (output, out_mask, out_shape) = pool_2d(&input, &mask, &[2, 2], &cfg);
        assert_eq!(out_shape, vec![1, 1]);
        assert_eq!(output, vec![0.0]);
        assert_eq!(out_mask, vec![0]);
    }

    #[test]
    fn stride_1_produces_larger_output() {
        let input: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        let mask = vec![1u8; 9];
        let cfg = pool_cfg(PoolKernel::Mean, 2, 1);

        let (_, _, out_shape) = pool_2d(&input, &mask, &[3, 3], &cfg);
        // (3-2)/1+1=2 in each dim
        assert_eq!(out_shape, vec![2, 2]);
    }

    #[test]
    fn kernel_larger_than_input_gives_empty() {
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let mask = vec![1u8; 4];
        let cfg = pool_cfg(PoolKernel::Mean, 3, 1);

        let (output, _, out_shape) = pool_2d(&input, &mask, &[2, 2], &cfg);
        assert_eq!(out_shape, vec![0, 0]);
        assert!(output.is_empty());
    }

    #[test]
    fn pool_2d_into_matches_allocating_variant() {
        let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let mask = vec![1u8; 16];
        let cfg = pool_cfg(PoolKernel::Mean, 2, 2);

        let (expected_output, expected_mask, expected_shape) =
            pool_2d(&input, &mask, &[4, 4], &cfg);
        let (out_h, out_w) = pool_2d_output_shape(&[4, 4], &cfg);
        let mut output = vec![123.0f32; out_h * out_w];
        let mut output_mask = vec![9u8; out_h * out_w];
        let actual_shape =
            pool_2d_into(&input, &mask, &[4, 4], &cfg, &mut output, &mut output_mask).unwrap();

        assert_eq!(actual_shape, (expected_shape[0], expected_shape[1]));
        assert_eq!(output, expected_output);
        assert_eq!(output_mask, expected_mask);
    }

    #[test]
    fn pool_2d_into_rejects_undersized_output() {
        let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let mask = vec![1u8; 16];
        let cfg = pool_cfg(PoolKernel::Mean, 2, 2);
        // 4x4 with kernel 2 stride 2 → 2x2 = 4 cells, but we give only 2
        let mut output = vec![0.0f32; 2];
        let mut output_mask = vec![0u8; 2];
        let result =
            pool_2d_into(&input, &mask, &[4, 4], &cfg, &mut output, &mut output_mask);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("output buffer too small"), "got: {msg}");
    }
}
