use rupix_abi::{IvRenderOut, IvRenderReq};
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("wmf path");
    let data = std::fs::read(path).expect("read");
    let req = IvRenderReq {
        target_w: 2000,
        target_h: 2000,
        ..Default::default()
    };
    let mut info = IvRenderOut::default();
    let q = pro_wmf::render_impl(
        data.as_ptr(),
        data.len(),
        &req,
        std::ptr::null_mut(),
        0,
        &mut info,
        std::ptr::null(),
    );
    assert_eq!(q, 0);
    let mut out = vec![0u8; info.bytes_needed as usize];
    let mut best = u128::MAX;
    let mut hash = 0u64;
    for _ in 0..8 {
        let t = Instant::now();
        let code = pro_wmf::render_impl(
            data.as_ptr(),
            data.len(),
            &req,
            out.as_mut_ptr(),
            out.len(),
            &mut info,
            std::ptr::null(),
        );
        let us = t.elapsed().as_micros();
        assert_eq!(code, 0);
        best = best.min(us);
        hash = out.chunks_exact(4).fold(1469598103934665603u64, |h, p| {
            h.wrapping_mul(1099511628211) ^ u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as u64
        });
    }
    println!(
        "target={}x{} bytes={} best={}us hash={:016x}",
        info.actual_w,
        info.actual_h,
        out.len(),
        best,
        hash
    );
}
