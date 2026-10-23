//! `Bytes`: a byte array across UniFFI in one memcpy each way.


/// Bytes across UniFFI in one memcpy each way. Same wire form and foreign type as `Vec<u8>`
/// (Kotlin `ByteArray`), but UniFFI lifts/lowers a `Vec<u8>` element by element, which costs
/// milliseconds per video frame on a phone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl std::ops::Deref for Bytes {
    type Target = Vec<u8>;
    fn deref(&self) -> &Vec<u8> {
        &self.0
    }
}

impl std::ops::DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }
}

// SAFETY: the same serialized form (`i32` BE length, then the bytes) and metadata as UniFFI's
// own `Vec<u8>`, so the foreign side treats it exactly like one.
unsafe impl uniffi::FfiConverter<crate::UniFfiTag> for Bytes {
    type FfiType = uniffi::RustBuffer;

    fn lower(obj: Self) -> uniffi::RustBuffer {
        let mut buf = Vec::with_capacity(4 + obj.0.len());
        Self::write(obj, &mut buf);
        uniffi::RustBuffer::from_vec(buf)
    }

    fn try_lift(v: uniffi::RustBuffer) -> uniffi::Result<Self> {
        let mut v = v.destroy_into_vec();
        let mut r = &v[..];
        let n = read_len(&mut r)?;
        if r.len() != n {
            uniffi::deps::anyhow::bail!("bytes: {} trailing", r.len() as i64 - n as i64);
        }
        v.drain(..4);
        Ok(Self(v))
    }

    fn write(obj: Self, buf: &mut Vec<u8>) {
        // Clamped, never a panic; a camera frame cannot reach 2 GiB, and the prefix and body stay consistent.
        let len = obj.0.len().min(i32::MAX as usize);
        buf.extend_from_slice(&(len as i32).to_be_bytes());
        buf.extend_from_slice(&obj.0[..len]);
    }

    fn try_read(buf: &mut &[u8]) -> uniffi::Result<Self> {
        let n = read_len(buf)?;
        uniffi::check_remaining(buf, n)?;
        let (b, rest) = buf.split_at(n);
        *buf = rest;
        Ok(Self(b.to_vec()))
    }

    const TYPE_ID_META: uniffi::MetadataBuffer = <Vec<u8> as uniffi::TypeId<crate::UniFfiTag>>::TYPE_ID_META;
}

uniffi::derive_ffi_traits!(local Bytes);

fn read_len(buf: &mut &[u8]) -> uniffi::Result<usize> {
    uniffi::check_remaining(buf, 4)?;
    let Some((n, rest)) = buf.split_first_chunk::<4>() else { uniffi::deps::anyhow::bail!("bytes: short length") };
    *buf = rest;
    Ok(usize::try_from(i32::from_be_bytes(*n))?)
}
