//! Immutable original host attestations, separate from unsigned legacy Ack rows.
use super::*;
use crate::group::doc::DocId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredAttestation {
    pub doc: DocId,
    pub bytes: Vec<u8>,
}

impl GroupStore {
    pub(crate) fn storage_attestation(
        &self,
        gid: &str,
        digest: &str,
    ) -> Result<Option<StoredAttestation>, Error> {
        self.db
            .get(&format!("proof/{gid}/{digest}"))
            .map_err(io)?
            .map(|row| bincode::deserialize(&row).map_err(|e| Error::Io(e.to_string())))
            .transpose()
    }
    pub(crate) fn storage_attestation_page(
        &self,
        gid: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, StoredAttestation)>, Error> {
        let prefix = format!("proof/{gid}/");
        let cursor = after.map(|id| format!("{prefix}{id}"));
        let mut rows = Vec::new();
        for key in self
            .db
            .keys_after(&prefix, cursor.as_deref(), limit)
            .map_err(io)?
        {
            let digest = key
                .strip_prefix(&prefix)
                .ok_or_else(|| Error::Io("bad proof key".into()))?;
            let row = self
                .storage_attestation(gid, digest)?
                .ok_or_else(|| Error::Io("missing proof row".into()))?;
            rows.push((digest.into(), row));
        }
        Ok(rows)
    }
    pub(crate) fn attestation_ack(
        &self,
        gid: &str,
        doc: &DocId,
        did: &str,
        dev: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, Error> {
        self.db.get(&ack_key(gid, doc, did, dev)).map_err(io)
    }
    /// Original proof, UI Ack projection and any debt clear commit together.
    /// Only the verifier under the importer lock may call this adapter.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn commit_storage_attestation(
        &self,
        gid: &str,
        digest: &str,
        row: &StoredAttestation,
        did: &str,
        dev: &[u8; 32],
        ack: &[u8],
        meta: &GroupMeta,
    ) -> Result<(), Error> {
        #[cfg(test)]
        if self
            .fail_next_meta_write
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(Error::Io(
                "injected receipt metadata transaction failure".into(),
            ));
        }
        let proof = bincode::serialize(row).map_err(|e| Error::Io(e.to_string()))?;
        let ops = [
            self.db
                .put_op(&format!("proof/{gid}/{digest}"), &proof)
                .map_err(io)?,
            self.db
                .put_op(&ack_key(gid, &row.doc, did, dev), ack)
                .map_err(io)?,
            self.db
                .put_op(&format!("grp/{gid}"), &json(meta))
                .map_err(io)?,
        ];
        self.apply_append(&ops)
    }
}
fn ack_key(gid: &str, doc: &DocId, did: &str, dev: &[u8; 32]) -> String {
    match doc {
        DocId::Meta => format!("ack/{gid}/{did}/{}", hex::encode(dev)),
        DocId::Day(day) => format!("ackd/{gid}/{day}/{did}/{}", hex::encode(dev)),
    }
}
