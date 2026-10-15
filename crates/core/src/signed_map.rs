//! Shared native grow-only binary-map operation inspection. Content codecs and
//! authorization remain caller policy; this never reauthors or resigns values.
use loro::{
    ContainerID, ContainerType, JsonMapOp, JsonOpContent, LoroDoc, LoroValue, VersionVector,
};

pub(crate) fn inserts<T, E: Clone>(
    doc: &LoroDoc,
    from: &VersionVector,
    to: &VersionVector,
    root_name: &str,
    structure_error: E,
    mut decode: impl FnMut(&str, &[u8], u64, i32) -> Result<T, E>,
) -> Result<Vec<T>, E> {
    let root = ContainerID::new_root(root_name, ContainerType::Map);
    let mut out = Vec::new();
    for change in doc
        .export_json_updates_without_peer_compression(from, to)
        .changes
    {
        for op in change.ops {
            if op.container != root {
                return Err(structure_error.clone());
            }
            let JsonOpContent::Map(JsonMapOp::Insert { key, value }) = &op.content else {
                return Err(structure_error.clone());
            };
            let LoroValue::Binary(bytes) = value else {
                return Err(structure_error.clone());
            };
            out.push(decode(key, bytes, change.id.peer, op.counter)?);
        }
    }
    Ok(out)
}
