use std::time::Instant;

use super::*;

#[test]
fn bytes_cross_like_vec() {
    use uniffi::{Lift, Lower, RustBuffer};
    type T = crate::UniFfiTag;
    let data: Vec<u8> = (0..640 * 480 * 3 / 2).map(|i| i as u8).collect();
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        for _ in 0..20 {
            f();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let wire = <Vec<u8> as Lower<T>>::lower_into_rust_buffer(data.clone()).destroy_into_vec();
    assert_eq!(<Bytes as Lower<T>>::lower_into_rust_buffer(Bytes(data.clone())).destroy_into_vec(), wire);
    assert_eq!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap().0, data);
    assert!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire[..wire.len() - 1].to_vec())).is_err());
    let frame = VideoFrame { width: 640, height: 480, data: Bytes(data.clone()), seq: 1 };
    let mut r = &<VideoFrame as Lower<T>>::lower_into_rust_buffer(frame).destroy_into_vec()[..];
    assert_eq!(<VideoFrame as Lift<T>>::try_read(&mut r).unwrap().data.0, data);
    let lift_vec = time(&mut || drop(<Vec<u8> as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
    let lift_bytes = time(&mut || drop(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
    let lower_vec = time(&mut || <Vec<u8> as Lower<T>>::lower(data.clone()).destroy());
    let lower_bytes = time(&mut || <Bytes as Lower<T>>::lower(Bytes(data.clone())).destroy());
    println!("640x480 I420 ms (incl. a clone): lift Vec<u8> {lift_vec:.3} Bytes {lift_bytes:.3}, lower Vec<u8> {lower_vec:.3} Bytes {lower_bytes:.3}");
}
