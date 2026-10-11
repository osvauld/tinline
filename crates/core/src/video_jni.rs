//! Android frame calls that skip UniFFI (`com.osvauld.p2p.VideoJni`): the camera's direct
//! `ByteBuffer`s are read in place and copied once into the encoder's slot; the RGBA pull writes
//! into a Kotlin-owned direct `ByteBuffer`. UniFFI would copy every frame through a
//! `ByteArray` and a `RustBuffer` (docs/video.md, "Frames across the FFI").
//!
//! Safety: the node is found by `Node::video_frame_handle` (never a pointer from Kotlin);
//! buffers are only read within the address and capacity the JVM reports for them, and only
//! during the call, while Kotlin keeps them alive (the `ImageProxy` is open). Bad arguments
//! give false / an error code, never a crash.

use std::panic::{AssertUnwindSafe, catch_unwind};

use jni::JNIEnv;
use jni::objects::{JByteBuffer, JClass, JIntArray};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jlong};

use crate::video_call::{Pulled, chroma_spans, frame_port};

/// A direct buffer's memory: (address, capacity). None for a null or non-direct buffer.
fn region(env: &JNIEnv, b: &JByteBuffer) -> Option<(*mut u8, usize)> {
    if b.as_raw().is_null() {
        return None;
    }
    let p = env.get_direct_buffer_address(b).ok()?;
    let n = env.get_direct_buffer_capacity(b).ok()?;
    Some((p, n))
}

/// # Safety
/// `p..p+n` must be readable for the returned lifetime.
unsafe fn bytes<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if n == 0 { &[] } else { unsafe { std::slice::from_raw_parts(p, n) } }
}

fn uint(x: jint) -> Option<u32> {
    u32::try_from(x).ok()
}

/// `VideoJni.pushYuv(handle, width, height, rotation, y, yRowStride, u, v, uvRowStride,
/// uvPixelStride, fullRange): Boolean`, the planes being `ImageProxy.planes[i].buffer`
/// (whole buffers, from their start). Like `Node::push_video_frame_yuv`; with pixel stride 2
/// and U, V one byte apart (NV12/NV21 views of one plane) the plane is read once.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_osvauld_p2p_VideoJni_pushYuv<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    width: jint,
    height: jint,
    rotation: jint,
    y: JByteBuffer<'l>,
    y_row_stride: jint,
    u: JByteBuffer<'l>,
    v: JByteBuffer<'l>,
    uv_row_stride: jint,
    uv_pixel_stride: jint,
    full_range: jboolean,
) -> jboolean {
    let run = || -> Option<bool> {
        let node = frame_port(handle as u64)?;
        let (w, h, rot) = (uint(width)?, uint(height)?, uint(rotation)?);
        let (ys, uvs, ps) = (uint(y_row_stride)? as usize, uint(uv_row_stride)? as usize, uint(uv_pixel_stride)? as usize);
        let (yp, yn) = region(&env, &y)?;
        let (up, un) = region(&env, &u)?;
        let (vp, vn) = region(&env, &v)?;
        let ((ua, ul), (va, vl)) = chroma_spans((up as usize, un), (vp as usize, vn), ps);
        // SAFETY: each region is a live direct buffer as the JVM reports it; an interleaved
        // plane (`chroma_spans`) is the union of two such buffers that overlap, so it is
        // contiguous valid memory too.
        let (y, u, v) = unsafe { (bytes(yp, yn), bytes(ua as *const u8, ul), bytes(va as *const u8, vl)) };
        Some(node.push_yuv_planes(w, h, rot, y, ys, u, v, uvs, ps, full_range != JNI_FALSE))
    };
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(Some(true)) => JNI_TRUE,
        _ => JNI_FALSE,
    }
}

/// `VideoJni.pullRgba(handle, dst, info): Int`: the newest decoded frame not pulled yet as packed
/// RGBA at the start of the direct buffer `dst` (`Bitmap.copyPixelsFromBuffer` order).
/// 1 = a frame, `info` = [width, height, seq (low 32 bits)]; 0 = none; -1 = `dst` is under
/// `width * height * 4` bytes, `info` = [width, height] and the frame stays for the next call;
/// -2 = bad arguments (unknown handle, `dst` not direct, `info` shorter than 3).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_osvauld_p2p_VideoJni_pullRgba<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    dst: JByteBuffer<'l>,
    info: JIntArray<'l>,
) -> jint {
    let run = || -> Option<jint> {
        let node = frame_port(handle as u64)?;
        if info.as_raw().is_null() || env.get_array_length(&info).ok()? < 3 {
            return None;
        }
        let (p, n) = region(&env, &dst)?;
        // SAFETY: a live direct buffer of `n` bytes, written only during this call.
        let out = if n == 0 { &mut [][..] } else { unsafe { std::slice::from_raw_parts_mut(p, n) } };
        let (code, vals) = match node.pull_rgba_into(out) {
            Pulled::None => return Some(0),
            Pulled::TooSmall { width, height } => (-1, [width as jint, height as jint, 0]),
            Pulled::Frame { width, height, seq } => (1, [width as jint, height as jint, seq as jint]),
        };
        env.set_int_array_region(&info, 0, &vals).ok()?;
        Some(code)
    };
    catch_unwind(AssertUnwindSafe(run)).ok().flatten().unwrap_or(-2)
}
